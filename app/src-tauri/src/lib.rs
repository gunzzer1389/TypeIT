// ---------------------------------------------------------------------------
// TypeIT — Rust core
//
// A frameless/transparent/always-on-top dictation window that:
//   * registers global shortcuts to show/hide ITS OWN window and to "type it";
//   * types the text you reviewed into whatever window you have focused, at a
//     speed you set in words-per-minute (standard dictation keystroke delivery,
//     via enigo).
//
// Scope note: it delivers keystrokes to the foreground window only — it does
// not enumerate, target, or lock onto specific other windows, does not capture
// the screen, and does not hide this process from the OS/task manager. Typing
// is driven by you: a button (which hides this window first so your previous
// app regains focus) or a global hotkey (after you click into your target).
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_log::{Target, TargetKind};
use tauri_plugin_opener::OpenerExt;

/// The most recent transcript the frontend pushed down, so the global "type it"
/// hotkey has something to type even when the webview isn't focused.
struct PendingText(Mutex<String>);

/// Typing speed as a words-per-minute RANGE (min, max), kept in sync from the
/// UI so the hotkey path types at the same pace as the button. Each character's
/// speed is drawn from this band, so the realized average lands near the middle
/// with natural, human-looking variation.
struct SpeedRange(Mutex<(u32, u32)>);

/// The still-untyped remainder from the last interrupted pass (Stop or focus
/// loss). Resume replays exactly this, so continuing never repeats or skips.
struct Resume(Mutex<String>);

/// Human-error rate: number of mistakes to make per sentence (0 = perfect).
struct Mistakes(Mutex<u32>);

/// A planned typo on a word: type `split` correct chars, then the `wrong`
/// chars, backspace them, and type the rest correctly.
struct Mistake {
    word_len: usize,
    split: usize,
    wrong: Vec<char>,
}

/// A believable QWERTY-neighbour typo for `c` (keeps case). Falls back to `c`.
fn nearby_key(c: char, rng: &mut Rng) -> char {
    let neighbors: &str = match c.to_ascii_lowercase() {
        'q' => "wa", 'w' => "qeas", 'e' => "wrsd", 'r' => "etdf", 't' => "rygf",
        'y' => "tugh", 'u' => "yihj", 'i' => "uojk", 'o' => "ipkl", 'p' => "ol",
        'a' => "qwsz", 's' => "awedxz", 'd' => "serfcx", 'f' => "drtgvc",
        'g' => "ftyhbv", 'h' => "gyujnb", 'j' => "huikmn", 'k' => "jiolm",
        'l' => "kop", 'z' => "asx", 'x' => "zsdc", 'c' => "xdfv", 'v' => "cfgb",
        'b' => "vghn", 'n' => "bhjm", 'm' => "njk",
        _ => return c,
    };
    let b = neighbors.as_bytes();
    let pick = b[(rng.next_u64() as usize) % b.len()] as char;
    if c.is_ascii_uppercase() { pick.to_ascii_uppercase() } else { pick }
}

/// Pick up to `mpl` of a sentence's words and record a typo plan for each.
fn choose_mistakes(
    words: &[(usize, usize)],
    chars: &[char],
    mpl: u32,
    rng: &mut Rng,
    plans: &mut HashMap<usize, Mistake>,
) {
    let n = words.len();
    if n == 0 {
        return;
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let k = (mpl as usize).min(n);
    for j in 0..k {
        // partial Fisher–Yates to pick distinct words
        let r = j + (rng.next_u64() as usize) % (n - j);
        idx.swap(j, r);
        let (start, len) = words[idx[j]];
        // correct-prefix length in [2, len-1]
        let split = (2 + (rng.next_u64() as usize) % (len - 2)).min(len - 1).max(1);
        let wrong = vec![nearby_key(chars[start + split], rng)];
        plans.insert(start, Mistake { word_len: len, split, wrong });
    }
}

/// Plan up to `mpl` typos per sentence (sentences split on . ! ?). Only words
/// of 4+ letters are eligible. Keyed by the word's start index in `chars`.
fn plan_mistakes(chars: &[char], mpl: u32, rng: &mut Rng) -> HashMap<usize, Mistake> {
    let mut plans = HashMap::new();
    if mpl == 0 {
        return plans;
    }
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            let len = i - start;
            if len >= 4 {
                words.push((start, len));
            }
        } else {
            if matches!(chars[i], '.' | '!' | '?') {
                choose_mistakes(&words, chars, mpl, rng, &mut plans);
                words.clear();
            }
            i += 1;
        }
    }
    choose_mistakes(&words, chars, mpl, rng, &mut plans);
    plans
}

/// Store the remainder after a pass: keep it if interrupted, clear if finished.
fn remember_resume(app: &tauri::AppHandle, outcome: &TypeOutcome) {
    if let Some(state) = app.try_state::<Resume>() {
        if let Ok(mut guard) = state.0.lock() {
            *guard = if outcome.reason == "done" {
                String::new()
            } else {
                outcome.remaining.clone()
            };
        }
    }
}

/// Sensible bounds: below ~10 wpm is uselessly slow, and 1000 wpm is already
/// close to "as fast as the target app can keep up".
const WPM_MIN: u32 = 10;
const WPM_MAX: u32 = 1000;
const DEFAULT_MIN: u32 = 180;
const DEFAULT_MAX: u32 = 220;

/// Clamp both ends to the allowed bounds and make sure min <= max.
fn normalize_range(min: u32, max: u32) -> (u32, u32) {
    let a = min.clamp(WPM_MIN, WPM_MAX);
    let b = max.clamp(WPM_MIN, WPM_MAX);
    (a.min(b), a.max(b))
}

/// Per-character pause for a given wpm. Uses the standard 5-characters-per-word
/// convention: chars/min = wpm * 5, so ms/char = 60_000 / (wpm * 5) = 12_000/wpm.
fn per_char_delay(wpm: u32) -> Duration {
    let wpm = wpm.clamp(WPM_MIN, WPM_MAX);
    Duration::from_millis((12_000 / wpm) as u64)
}

/// Tiny dependency-free PRNG (xorshift64), seeded from the clock. Good enough
/// for jittering typing speed — this is not security-sensitive randomness.
struct Rng(u64);
impl Rng {
    fn seeded() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng(seed | 1) // never zero
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Uniform integer in [lo, hi] inclusive.
    fn in_range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (u64::from(hi - lo) + 1)) as u32
    }
}

/// True while a typing operation is running, so a second trigger (button +
/// hotkey, or a double hotkey press) can't start a concurrent stream — that
/// interleaves characters and scrambles the output.
static TYPING: AtomicBool = AtomicBool::new(false);

/// Set to request the in-flight typing loop to stop (Stop button / hotkey).
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Clears the TYPING flag on drop, so it is released even on early return.
struct TypingGuard;
impl Drop for TypingGuard {
    fn drop(&mut self) {
        TYPING.store(false, Ordering::SeqCst);
    }
}

/// Result of a typing pass, sent to the frontend.
///   reason: "done" (finished), "stopped" (Stop), or "focus_lost" (you clicked
///   into another window). `remaining` is the still-untyped text, so the UI can
///   put it back in the box and resume from there.
#[derive(Clone, Serialize)]
struct TypeOutcome {
    typed: usize,
    reason: String,
    remaining: String,
}

/// The most recent non-TypeIT foreground window (HWND as isize), updated by a
/// watcher thread on Windows. Used to hand focus back to your target app so
/// TypeIT can stay visible (showing Stop) while it types.
static LAST_EXTERNAL: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// (foreground window HWND as isize, its process id). 0/0 = unknown.
#[cfg(windows)]
fn foreground() -> (isize, u32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return (0, 0);
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        (hwnd.0 as isize, pid)
    }
}
#[cfg(target_os = "macos")]
fn foreground() -> (isize, u32) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    // Frontmost app's pid via NSWorkspace. We key everything off the pid.
    unsafe {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return (0, 0);
        }
        let app: *mut AnyObject = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return (0, 0);
        }
        let pid: i32 = msg_send![app, processIdentifier];
        (pid as isize, pid as u32)
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
fn foreground() -> (isize, u32) {
    (0, 0) // focus-aware features unavailable on this platform
}

/// Process id of the focused window. We compare by *process* (not window
/// handle) so the target app's own popups — autocomplete, emoji pickers — don't
/// look like "you switched away".
fn foreground_pid() -> u32 {
    foreground().1
}

/// Try to raise the target app to the foreground. True if it succeeded.
#[cfg(windows)]
fn set_foreground(hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    if hwnd == 0 {
        return false;
    }
    unsafe { SetForegroundWindow(HWND(hwnd as *mut core::ffi::c_void)).as_bool() }
}
#[cfg(target_os = "macos")]
fn set_foreground(pid: isize) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    if pid == 0 {
        return false;
    }
    unsafe {
        let app: *mut AnyObject = msg_send![
            class!(NSRunningApplication),
            runningApplicationWithProcessIdentifier: pid as i32
        ];
        if app.is_null() {
            return false;
        }
        // NSApplicationActivateIgnoringOtherApps = 1 << 1 (harmless no-op on
        // macOS 14+, still activates the app).
        let options: usize = 1 << 1;
        let ok: bool = msg_send![app, activateWithOptions: options];
        ok
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
fn set_foreground(_id: isize) -> bool {
    false
}

/// Before typing (button/resume path): try to focus the target app so TypeIT
/// can stay visible with a working Stop button. Returns true if the target now
/// has focus (stay visible); false means the caller should hide instead.
fn bring_target_forward() -> bool {
    let target = LAST_EXTERNAL.load(Ordering::SeqCst);
    if target != 0 && set_foreground(target) {
        std::thread::sleep(Duration::from_millis(150));
        // Confirm focus actually left us before we start typing.
        if foreground().1 != std::process::id() {
            return true;
        }
    }
    false
}

/// Synthesize `text` into the focused window one character at a time. Each
/// character's pause corresponds to a wpm picked uniformly in [min, max], so
/// the overall pace varies naturally and averages near the band's midpoint.
/// Returns the number of characters actually typed. If it's less than the
/// text length, typing was cancelled (Stop) — the caller can resume with the
/// remaining slice. Character counting is by Unicode scalar (`chars`), matching
/// the frontend's `Array.from(text)` so resume slicing lines up.
fn type_string_range(
    text: &str,
    min_wpm: u32,
    max_wpm: u32,
    mpl: u32,
) -> Result<TypeOutcome, String> {
    if text.trim().is_empty() {
        return Err("Nothing to type".into());
    }
    // Refuse to start if another typing pass is already in flight.
    if TYPING.swap(true, Ordering::SeqCst) {
        log::warn!("type request ignored: already typing");
        return Err("Already typing".into());
    }
    let _guard = TypingGuard; // releases TYPING when this function returns
    CANCEL.store(false, Ordering::SeqCst); // fresh run

    let chars: Vec<char> = text.chars().collect();
    let outcome = |typed: usize, reason: &str| TypeOutcome {
        typed,
        reason: reason.to_string(),
        remaining: chars[typed.min(chars.len())..].iter().collect(),
    };

    let (lo, hi) = normalize_range(min_wpm, max_wpm);
    // The app (process) the user is typing into. If focus moves to a different
    // process, stop — but same-process popups don't count.
    let target_pid = foreground_pid();
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
        log::error!("enigo init failed: {e}");
        e.to_string()
    })?;
    let mut rng = Rng::seeded();
    let plans = plan_mistakes(&chars, mpl, &mut rng);
    let mut buf = [0u8; 4];

    // Type one char, respecting the wpm band.
    macro_rules! put {
        ($ch:expr) => {{
            let s = ($ch).encode_utf8(&mut buf);
            enigo.text(s).map_err(|e| {
                log::error!("enigo type failed: {e}");
                e.to_string()
            })?;
            std::thread::sleep(per_char_delay(rng.in_range(lo, hi)));
        }};
    }

    let mut i = 0usize;
    while i < chars.len() {
        if CANCEL.load(Ordering::SeqCst) {
            log::info!("typing stopped after {i} chars");
            return Ok(outcome(i, "stopped"));
        }
        // Only check focus at unit boundaries (not mid-word) so a typo+fix stays
        // atomic; words are short, so cancellation is still responsive.
        if target_pid != 0 && foreground_pid() != target_pid {
            log::info!("focus left target app; paused after {i} chars");
            return Ok(outcome(i, "focus_lost"));
        }

        if let Some(plan) = plans.get(&i) {
            let word: Vec<char> = chars[i..i + plan.word_len].to_vec();
            // correct prefix
            for &c in &word[..plan.split] {
                put!(c);
            }
            // the mistake: wrong chars
            for &c in &plan.wrong {
                put!(c);
            }
            // a beat to "notice" it, then backspace and correct — this is what
            // slows the word down a touch and reads as human.
            std::thread::sleep(Duration::from_millis(180 + (rng.next_u64() % 220)));
            for _ in 0..plan.wrong.len() {
                enigo
                    .key(Key::Backspace, Direction::Click)
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(per_char_delay(rng.in_range(lo, hi)));
            }
            for &c in &word[plan.split..] {
                put!(c);
            }
            i += plan.word_len;
        } else {
            put!(chars[i]);
            i += 1;
        }
    }
    log::info!("typed {i} chars ({} typos) at {lo}-{hi} wpm", plans.len());
    Ok(outcome(i, "done"))
}

/// Flip always-on-top from the UI. `window` is always this app's own window.
#[tauri::command]
fn set_always_on_top(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    window.set_always_on_top(enabled).map_err(|e| e.to_string())
}

/// Open a URL in the user's default browser (the "Get a Deepgram API key" link).
/// Custom commands aren't ACL-gated, so this needs no capability entry.
#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|e| e.to_string())
}

/// Frontend keeps this in sync with the transcript box (debounced) so the
/// global type-it hotkey can fire without the webview being focused.
#[tauri::command]
fn set_pending_text(text: String, state: tauri::State<PendingText>) {
    if let Ok(mut guard) = state.0.lock() {
        *guard = text;
    }
}

/// Frontend keeps this in sync with the speed-range control.
#[tauri::command]
fn set_speed_range(min_wpm: u32, max_wpm: u32, state: tauri::State<SpeedRange>) {
    if let Ok(mut guard) = state.0.lock() {
        *guard = normalize_range(min_wpm, max_wpm);
    }
}

/// Frontend keeps this in sync with the mistakes-per-sentence control.
#[tauri::command]
fn set_mistakes(mpl: u32, state: tauri::State<Mistakes>) {
    if let Ok(mut guard) = state.0.lock() {
        *guard = mpl.min(20);
    }
}

/// Button path: hide our own window so the app you had focused regains focus,
/// give the OS a beat to move focus back, then type at `wpm`. Runs on a
/// blocking thread so neither the sleep nor the typing freezes the UI.
#[tauri::command]
async fn type_text(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    text: String,
    min_wpm: u32,
    max_wpm: u32,
    mpl: u32,
) -> Result<TypeOutcome, String> {
    let win = window.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        // Prefer keeping our window visible (so the Stop button is usable) by
        // focusing the target app. If that can't be confirmed, fall back to
        // hiding so focus reliably returns to the previous app.
        if bring_target_forward() {
            std::thread::sleep(Duration::from_millis(120));
        } else {
            let _ = win.hide();
            std::thread::sleep(Duration::from_millis(400));
        }
        type_string_range(&text, min_wpm, max_wpm, mpl)
    })
    .await
    .map_err(|e| e.to_string())??;
    remember_resume(&app, &outcome);
    // If focus wandered off, bring TypeIT back so the user sees the pause and
    // the remaining text (with a hint to click into their app and resume).
    if outcome.reason == "focus_lost" {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(outcome)
}

/// Resume the last interrupted pass: type only the stored remainder.
#[tauri::command]
async fn resume_typing(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    min_wpm: u32,
    max_wpm: u32,
) -> Result<TypeOutcome, String> {
    let text = app
        .try_state::<Resume>()
        .and_then(|s| s.0.lock().ok().map(|g| g.clone()))
        .unwrap_or_default();
    if text.trim().is_empty() {
        return Err("Nothing to resume".into());
    }
    let mpl = app
        .try_state::<Mistakes>()
        .and_then(|s| s.0.lock().ok().map(|g| *g))
        .unwrap_or(0);
    let win = window.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        if bring_target_forward() {
            std::thread::sleep(Duration::from_millis(120));
        } else {
            let _ = win.hide();
            std::thread::sleep(Duration::from_millis(400));
        }
        type_string_range(&text, min_wpm, max_wpm, mpl)
    })
    .await
    .map_err(|e| e.to_string())??;
    remember_resume(&app, &outcome);
    if outcome.reason == "focus_lost" {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(outcome)
}

/// Request the in-flight typing to stop (Stop button; also the Stop hotkey).
#[tauri::command]
fn stop_typing() {
    CANCEL.store(true, Ordering::SeqCst);
    log::info!("stop_typing requested");
}

/// Fully quit the app (red titlebar button; also the Ctrl+Shift+Q hotkey).
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    log::info!("quit requested");
    app.exit(0);
}

/// Application bootstrap. Called from main.rs.
pub fn run() {
    // Show/hide this window. Ctrl+Shift+Space (plain Ctrl+Space is the IME
    // switcher on Windows and macOS, so we add Shift).
    let toggle = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    // Type the pending transcript into the foreground app. Use this after you
    // click into your target (email, chat, comment box).
    let type_it = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Enter);
    // Stop the in-flight typing from anywhere (the window is hidden while the
    // button types, so Stop has to be global).
    let stop_it = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Backspace);
    // Resume the last interrupted pass (types only the remainder).
    let resume_it = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
    // Fully quit the app from anywhere.
    let quit_hk = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyQ);

    // Log panics to the log file too, so a startup crash leaves a trace.
    std::panic::set_hook(Box::new(|info| {
        log::error!("PANIC: {info}");
        eprintln!("PANIC: {info}");
    }));

    tauri::Builder::default()
        // Must be the FIRST plugin. If a second copy of TypeIT is launched,
        // focus the running window instead of opening another instance.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            log::info!("second instance launched; focusing existing window");
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    Target::new(TargetKind::Stdout),
                    // Windows: %APPDATA%\com.typeit.app\logs\typeit.log
                    // macOS:   ~/Library/Logs/com.typeit.app/typeit.log
                    Target::new(TargetKind::LogDir {
                        file_name: Some("typeit".into()),
                    }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if shortcut == &toggle {
                        if let Some(win) = app.get_webview_window("main") {
                            match win.is_visible() {
                                Ok(true) => {
                                    let _ = win.hide();
                                }
                                _ => {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                        }
                    } else if shortcut == &stop_it {
                        CANCEL.store(true, Ordering::SeqCst);
                        log::info!("stop via hotkey");
                    } else if shortcut == &quit_hk {
                        log::info!("quit via hotkey");
                        app.exit(0);
                    } else if shortcut == &resume_it {
                        let text = app
                            .try_state::<Resume>()
                            .and_then(|s| s.0.lock().ok().map(|g| g.clone()))
                            .unwrap_or_default();
                        if text.trim().is_empty() {
                            log::info!("resume hotkey: nothing to resume");
                        } else {
                            let (lo, hi) = app
                                .try_state::<SpeedRange>()
                                .and_then(|s| s.0.lock().ok().map(|g| *g))
                                .unwrap_or((DEFAULT_MIN, DEFAULT_MAX));
                            let mpl = app
                                .try_state::<Mistakes>()
                                .and_then(|s| s.0.lock().ok().map(|g| *g))
                                .unwrap_or(0);
                            let _ = app.emit("typing:start", ());
                            let app_handle = app.clone();
                            std::thread::spawn(move || match type_string_range(&text, lo, hi, mpl) {
                                Ok(outcome) => {
                                    remember_resume(&app_handle, &outcome);
                                    if outcome.reason == "focus_lost" {
                                        if let Some(win) = app_handle.get_webview_window("main") {
                                            let _ = win.show();
                                            let _ = win.set_focus();
                                        }
                                    }
                                    let _ = app_handle.emit("type-outcome", outcome);
                                }
                                Err(e) => log::warn!("hotkey resume: {e}"),
                            });
                        }
                    } else if shortcut == &type_it {
                        // Target app is already focused (you clicked into it),
                        // so type straight away — no hide needed. Do it on its
                        // own thread so the paced typing never blocks the app.
                        let text = app
                            .try_state::<PendingText>()
                            .and_then(|s| s.0.lock().ok().map(|g| g.clone()))
                            .unwrap_or_default();
                        let (lo, hi) = app
                            .try_state::<SpeedRange>()
                            .and_then(|s| s.0.lock().ok().map(|g| *g))
                            .unwrap_or((DEFAULT_MIN, DEFAULT_MAX));
                        let mpl = app
                            .try_state::<Mistakes>()
                            .and_then(|s| s.0.lock().ok().map(|g| *g))
                            .unwrap_or(0);
                        let _ = app.emit("typing:start", ());
                        let app_handle = app.clone();
                        std::thread::spawn(move || match type_string_range(&text, lo, hi, mpl) {
                            Ok(outcome) => {
                                remember_resume(&app_handle, &outcome);
                                if outcome.reason == "focus_lost" {
                                    if let Some(win) = app_handle.get_webview_window("main") {
                                        let _ = win.show();
                                        let _ = win.set_focus();
                                    }
                                }
                                let _ = app_handle.emit("type-outcome", outcome);
                            }
                            Err(e) => log::warn!("hotkey type: {e}"),
                        });
                    }
                })
                .build(),
        )
        .manage(PendingText(Mutex::new(String::new())))
        .manage(SpeedRange(Mutex::new((DEFAULT_MIN, DEFAULT_MAX))))
        .manage(Resume(Mutex::new(String::new())))
        .manage(Mistakes(Mutex::new(0)))
        .setup(move |app| {
            log::info!("TypeIT setup: starting");

            // Windows/macOS: continuously remember the last non-TypeIT
            // foreground app, so typing can hand focus back to it and stay
            // visible (and detect when focus leaves the target).
            #[cfg(any(windows, target_os = "macos"))]
            std::thread::spawn(|| {
                let our_pid = std::process::id();
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let (hwnd, pid) = foreground();
                    if hwnd != 0 && pid != 0 && pid != our_pid {
                        LAST_EXTERNAL.store(hwnd, Ordering::SeqCst);
                    }
                }
            });

            // macOS: run as a menu-bar (accessory) app — no Dock icon, reached
            // via the tray and the global hotkey. (Windows/Linux use the
            // window's skipTaskbar flag in tauri.conf.json.) Not concealment:
            // it's a normal process with a visible tray icon.
            #[cfg(target_os = "macos")]
            {
                use tauri::ActivationPolicy;
                app.set_activation_policy(ActivationPolicy::Accessory);
            }

            // Register shortcuts but DON'T let a clash abort startup — if
            // another app owns the combo, we log it and carry on so the
            // window still appears.
            match app.global_shortcut().register(toggle) {
                Ok(_) => log::info!("registered show/hide (Ctrl+Shift+Space)"),
                Err(e) => log::error!("show/hide shortcut not registered: {e}"),
            }
            match app.global_shortcut().register(type_it) {
                Ok(_) => log::info!("registered type-it (Ctrl+Shift+Enter)"),
                Err(e) => log::error!("type-it shortcut not registered: {e}"),
            }
            match app.global_shortcut().register(stop_it) {
                Ok(_) => log::info!("registered stop (Ctrl+Shift+Backspace)"),
                Err(e) => log::error!("stop shortcut not registered: {e}"),
            }
            match app.global_shortcut().register(resume_it) {
                Ok(_) => log::info!("registered resume (Ctrl+Shift+R)"),
                Err(e) => log::error!("resume shortcut not registered: {e}"),
            }
            match app.global_shortcut().register(quit_hk) {
                Ok(_) => log::info!("registered quit (Ctrl+Shift+Q)"),
                Err(e) => log::error!("quit shortcut not registered: {e}"),
            }

            // No tray / menu-bar icon by design: TypeIT is controlled purely by
            // global hotkeys (Ctrl+Shift+Space show/hide, Ctrl+Shift+Q quit) and
            // the red titlebar dot. It still appears in Task Manager / Activity
            // Monitor as a normal process.

            // Make sure the window is actually shown, unminimized, and focused.
            if let Some(win) = app.get_webview_window("main") {
                // Enforce no taskbar button even if the config flag is missed.
                let _ = win.set_skip_taskbar(true);
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
                log::info!("main window shown; visible={:?}", win.is_visible());
            } else {
                log::error!("main window 'main' NOT FOUND");
            }

            log::info!("TypeIT setup: done");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_always_on_top,
            open_url,
            set_pending_text,
            set_speed_range,
            set_mistakes,
            type_text,
            resume_typing,
            stop_typing,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running TypeIT");
}

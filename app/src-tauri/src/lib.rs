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

use std::sync::Mutex;
use std::time::Duration;

use enigo::{Enigo, Keyboard, Settings};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;
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

/// Synthesize `text` into the focused window one character at a time. Each
/// character's pause corresponds to a wpm picked uniformly in [min, max], so
/// the overall pace varies naturally and averages near the band's midpoint.
fn type_string_range(text: &str, min_wpm: u32, max_wpm: u32) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Nothing to type".into());
    }
    let (lo, hi) = normalize_range(min_wpm, max_wpm);
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
        log::error!("enigo init failed: {e}");
        e.to_string()
    })?;
    let mut rng = Rng::seeded();
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let wpm = rng.in_range(lo, hi);
        enigo.text(ch.encode_utf8(&mut buf)).map_err(|e| {
            log::error!("enigo type failed: {e}");
            e.to_string()
        })?;
        std::thread::sleep(per_char_delay(wpm));
    }
    log::info!("typed {} chars at {}-{} wpm", text.chars().count(), lo, hi);
    Ok(())
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

/// Button path: hide our own window so the app you had focused regains focus,
/// give the OS a beat to move focus back, then type at `wpm`. Runs on a
/// blocking thread so neither the sleep nor the typing freezes the UI.
#[tauri::command]
async fn type_text(
    window: tauri::WebviewWindow,
    text: String,
    min_wpm: u32,
    max_wpm: u32,
) -> Result<(), String> {
    let _ = window.hide();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(250));
        type_string_range(&text, min_wpm, max_wpm)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Application bootstrap. Called from main.rs.
pub fn run() {
    // Show/hide this window. Ctrl+Shift+Space (plain Ctrl+Space is the IME
    // switcher on Windows and macOS, so we add Shift).
    let toggle = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
    // Type the pending transcript into the foreground app. Use this after you
    // click into your target (email, chat, comment box).
    let type_it = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Enter);

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
                        std::thread::spawn(move || {
                            let _ = type_string_range(&text, lo, hi);
                        });
                    }
                })
                .build(),
        )
        .manage(PendingText(Mutex::new(String::new())))
        .manage(SpeedRange(Mutex::new((DEFAULT_MIN, DEFAULT_MAX))))
        .setup(move |app| {
            log::info!("TypeIT setup: starting");

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

            // System tray: gives the app a real Quit (window close only hides),
            // plus a Show/Hide toggle. Without a way to quit, an old instance
            // lingers in the background and locks its files against upgrades.
            let show_item = MenuItem::with_id(app, "show", "Show / Hide", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit TypeIT", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;
            if let Some(icon) = app.default_window_icon().cloned() {
                TrayIconBuilder::with_id("main-tray")
                    .icon(icon)
                    .tooltip("TypeIT")
                    .menu(&tray_menu)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "quit" => {
                            log::info!("quit from tray");
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(win) = app.get_webview_window("main") {
                                let visible = win.is_visible().unwrap_or(false);
                                if visible {
                                    let _ = win.hide();
                                } else {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                        }
                        _ => {}
                    })
                    .build(app)?;
                log::info!("tray icon created");
            } else {
                log::error!("no default window icon; tray not created");
            }

            // Make sure the window is actually shown, unminimized, and focused.
            if let Some(win) = app.get_webview_window("main") {
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
            type_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running TypeIT");
}

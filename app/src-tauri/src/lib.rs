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
use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// The most recent transcript the frontend pushed down, so the global "type it"
/// hotkey has something to type even when the webview isn't focused.
struct PendingText(Mutex<String>);

/// Typing speed in words-per-minute, kept in sync from the UI so the hotkey
/// path types at the same rate as the button.
struct Wpm(Mutex<u32>);

/// Sensible bounds: below ~10 wpm is uselessly slow, and 1000 wpm is already
/// close to "as fast as the target app can keep up".
const WPM_MIN: u32 = 10;
const WPM_MAX: u32 = 1000;
const WPM_DEFAULT: u32 = 240;

/// Per-character pause for a given wpm. Uses the standard 5-characters-per-word
/// convention: chars/min = wpm * 5, so ms/char = 60_000 / (wpm * 5) = 12_000/wpm.
fn per_char_delay(wpm: u32) -> Duration {
    let wpm = wpm.clamp(WPM_MIN, WPM_MAX);
    Duration::from_millis((12_000 / wpm) as u64)
}

/// Synthesize `text` into the focused window one character at a time, pausing
/// `delay` between characters so the typing runs at the requested speed.
fn type_string_at(text: &str, wpm: u32) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Nothing to type".into());
    }
    let delay = per_char_delay(wpm);
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        enigo.text(ch.encode_utf8(&mut buf)).map_err(|e| e.to_string())?;
        std::thread::sleep(delay);
    }
    Ok(())
}

/// Flip always-on-top from the UI. `window` is always this app's own window.
#[tauri::command]
fn set_always_on_top(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    window.set_always_on_top(enabled).map_err(|e| e.to_string())
}

/// Frontend keeps this in sync with the transcript box (debounced) so the
/// global type-it hotkey can fire without the webview being focused.
#[tauri::command]
fn set_pending_text(text: String, state: tauri::State<PendingText>) {
    if let Ok(mut guard) = state.0.lock() {
        *guard = text;
    }
}

/// Frontend keeps this in sync with the wpm control.
#[tauri::command]
fn set_wpm(wpm: u32, state: tauri::State<Wpm>) {
    if let Ok(mut guard) = state.0.lock() {
        *guard = wpm.clamp(WPM_MIN, WPM_MAX);
    }
}

/// Button path: hide our own window so the app you had focused regains focus,
/// give the OS a beat to move focus back, then type at `wpm`. Runs on a
/// blocking thread so neither the sleep nor the typing freezes the UI.
#[tauri::command]
async fn type_text(window: tauri::WebviewWindow, text: String, wpm: u32) -> Result<(), String> {
    let _ = window.hide();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(250));
        type_string_at(&text, wpm)
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

    tauri::Builder::default()
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
                        let wpm = app
                            .try_state::<Wpm>()
                            .and_then(|s| s.0.lock().ok().map(|g| *g))
                            .unwrap_or(WPM_DEFAULT);
                        std::thread::spawn(move || {
                            let _ = type_string_at(&text, wpm);
                        });
                    }
                })
                .build(),
        )
        .manage(PendingText(Mutex::new(String::new())))
        .manage(Wpm(Mutex::new(WPM_DEFAULT)))
        .setup(move |app| {
            app.global_shortcut().register(toggle)?;
            app.global_shortcut().register(type_it)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_always_on_top,
            set_pending_text,
            set_wpm,
            type_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running TypeIT");
}

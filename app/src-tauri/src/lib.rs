// ---------------------------------------------------------------------------
// TypeIT — Rust core
//
// A frameless/transparent/always-on-top dictation window that:
//   * registers global shortcuts to show/hide ITS OWN window and to "type it";
//   * types the text you reviewed into whatever window you have focused
//     (standard dictation keystroke delivery, via enigo).
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

/// Synthesize `text` as keystrokes into whatever window is currently focused.
fn type_string(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("Nothing to type".into());
    }
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
    enigo.text(text).map_err(|e| e.to_string())?;
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

/// Button path: hide our own window so the app you had focused regains focus,
/// give the OS a beat to move focus back, then type. Runs on a blocking thread
/// so the short sleep never freezes the UI.
#[tauri::command]
async fn type_text(window: tauri::WebviewWindow, text: String) -> Result<(), String> {
    let _ = window.hide();
    tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(Duration::from_millis(250));
        type_string(&text)
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
                        // so type straight away — no hide needed.
                        if let Some(state) = app.try_state::<PendingText>() {
                            let text = state.0.lock().map(|g| g.clone()).unwrap_or_default();
                            let _ = type_string(&text);
                        }
                    }
                })
                .build(),
        )
        .manage(PendingText(Mutex::new(String::new())))
        .setup(move |app| {
            app.global_shortcut().register(toggle)?;
            app.global_shortcut().register(type_it)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_always_on_top,
            set_pending_text,
            type_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running TypeIT");
}

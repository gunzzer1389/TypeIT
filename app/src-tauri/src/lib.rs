// ---------------------------------------------------------------------------
// Ghost Typer — visual shell (Rust core)
//
// Scope note: this backend only creates a frameless/transparent/always-on-top
// window and registers ONE global shortcut that shows/hides *this app's own*
// window. It does not simulate keystrokes into other applications, enumerate or
// target other windows, or alter how the OS captures/lists this process. It is
// the packaging layer for the UI study, nothing more.
// ---------------------------------------------------------------------------

use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Command exposed to the frontend to flip always-on-top from the UI if desired.
/// `window` is injected by Tauri; it is always this app's own window.
#[tauri::command]
fn set_always_on_top(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    // set_always_on_top maps to the native APIs (NSWindow level on macOS,
    // SetWindowPos HWND_TOPMOST on Windows). Return the error as a String so
    // it surfaces cleanly on the JS side.
    window
        .set_always_on_top(enabled)
        .map_err(|e| e.to_string())
}

/// Application bootstrap. Called from main.rs.
pub fn run() {
    // The show/hide accelerator. Ctrl+Shift+Space is chosen to avoid clashing
    // with the OS: plain Ctrl+Space is the input-method switcher on both
    // Windows and macOS, so we add Shift.
    let toggle = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                // Fires for every registered shortcut; we match ours and toggle
                // visibility of the main window. Pressed-state only, so it acts
                // once per keypress rather than on release too.
                .with_handler(move |app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed && shortcut == &toggle {
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
                    }
                })
                .build(),
        )
        .setup(move |app| {
            // Register the accelerator once the app handle exists.
            app.global_shortcut().register(toggle)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_always_on_top])
        .run(tauri::generate_context!())
        .expect("error while running Ghost Typer Shell");
}

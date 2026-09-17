// Thin entrypoint. On Windows in release, `windows_subsystem = "windows"`
// prevents a console window from flashing behind the GUI. All real logic
// lives in lib.rs so it can be shared/tested.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ghost_typer_shell_lib::run();
}

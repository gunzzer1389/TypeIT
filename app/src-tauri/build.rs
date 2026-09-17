// Tauri's build script: generates the context, embeds the frontend, and wires
// up permissions/capabilities at compile time. Must run before the crate builds.
fn main() {
    tauri_build::build();
}

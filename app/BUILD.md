# Building & running the TypeIT shell

This is the **visual shell** only: a frameless, translucent, always-on-top
window that renders the UI (`src/index.html`) with a working see-through
control and a global **Ctrl+Shift+Space** show/hide hotkey. It does **not**
type into other apps, target other windows, or hide from screen capture.

> **WSL / Linux note:** Tauri does not cross-compile GUI apps. A Windows `.exe`
> must be built **on Windows**, and a macOS `.app` **on a Mac**. You can't
> produce a Windows/Mac binary from WSL. Build on the OS you want to test.

---

## 1. Prerequisites

Install these on the machine you'll test on.

### Windows
1. **Microsoft C++ Build Tools** — https://visualstudio.microsoft.com/visual-cpp-build-tools/
   (select "Desktop development with C++").
2. **WebView2 Runtime** — preinstalled on Windows 11; on Windows 10 get the
   Evergreen runtime from Microsoft.
3. **Rust** — https://rustup.rs (run `rustup-init.exe`, accept defaults).
4. **Node.js 18+** — https://nodejs.org (for the Tauri CLI).

### macOS
1. **Xcode Command Line Tools:** `xcode-select --install`
2. **Rust:** `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
3. **Node.js 18+:** https://nodejs.org or `brew install node`

Verify: `rustc --version`, `cargo --version`, `node --version`.

---

## 2. Get the project onto that machine

```bash
git clone https://github.com/gunzzer1389/TypeIT.git
cd TypeIT/app
git checkout dev
```

## 3. Install the CLI and generate icons

```bash
npm install
npm run icons        # turns app-icon.png into src-tauri/icons/* (.ico, .icns, png)
```

The icon set is git-ignored/generated, so this step is required once before the
first build. Replace `app-icon.png` with your own 1024×1024 PNG anytime and
re-run `npm run icons`.

## 4. Run it (dev mode, hot-reloads the UI)

```bash
npm run dev
```

First run compiles the Rust deps (a few minutes); later runs are fast. A
frameless translucent widget appears, centered and on top. Try:

- **Ctrl+Shift+Space** — hide / show the window (works even when unfocused).
- **Gear icon** — show/hide the see-through panel.
- **Slider / presets** — change the glass opacity live.
- **Traffic lights** — red hides, yellow minimizes, green toggles always-on-top.
- **Drag** the empty part of the title bar to move the window.

## 5. Build a distributable

```bash
npm run build
```

Output lands in `src-tauri/target/release/bundle/` — an `.msi`/`.exe` on
Windows, a `.app`/`.dmg` on macOS.

---

## Troubleshooting

- **Window is opaque / not see-through:** transparency needs a compositor. On
  Windows ensure the app isn't in a high-contrast mode; on macOS it relies on
  `macOSPrivateApi` (already set in `tauri.conf.json`).
- **`glib`/`webkit` errors:** those are Linux-only deps — you're building on the
  wrong OS for your target. Build on Windows/macOS as noted above.
- **Global shortcut does nothing:** another app may already own
  Ctrl+Shift+Space; change the combo in `src-tauri/src/lib.rs`.

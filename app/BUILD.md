# Building & running TypeIT

A frameless, translucent, always-on-top **dictation helper**: speak, and your
words are transcribed (via Deepgram) into an editable box in the widget; review
them, then copy/send them where you need. It renders `src/index.html`, has a
working see-through control, and a global **Ctrl+Shift+Space** show/hide hotkey.
It records the mic only while dictation is on, and does **not** capture the
screen or hide itself from your system.

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

## 4b. Dictation setup (Deepgram + microphone)

- **API key:** open the **gear (settings)** panel → **Deepgram API key**, paste
  your key and hit **Save**. No key yet? The **Get a Deepgram API key** button
  there opens Deepgram in your browser. (Pressing Dictate with no key saved pops
  the settings panel open for you.) The key is stored in the webview's
  `localStorage` on this machine and sent only to Deepgram. (A more secure store
  — OS keychain via a Tauri command — is a planned follow-up.)
- **Microphone permission (macOS):** the app ships an
  `NSMicrophoneUsageDescription` (`src-tauri/Info.plist`), so macOS will prompt
  the first time you press Dictate. Approve it in the prompt (or later under
  System Settings → Privacy & Security → Microphone).
- **Microphone permission (Windows):** WebView2 requests mic access on first use;
  allow it. Ensure the mic isn't blocked under Settings → Privacy → Microphone.
- **No transcript?** Open the devtools console (right-click → Inspect in dev
  mode) — auth failures surface as a `Key rejected` status, mic issues as
  `Mic blocked`.

## 4c. Type-into-focused-window

Two ways to send the reviewed text to another app:

- **Type it button** — hides the TypeIT window (so the app you had focused comes
  back to the front) and types the transcript there.
- **Ctrl+Shift+Enter** (global) — click into your target first (email, chat,
  comment box), then press it; TypeIT types the current transcript there.

Keystrokes are synthesized with [`enigo`](https://crates.io/crates/enigo). Set
the pace with the **Typing speed** field (words per minute, 10–1000; −/+ to
nudge). Higher is faster but some apps drop characters if it's too fast; lower
is steadier. The setting is remembered between runs.

- **macOS:** typing requires **Accessibility** permission. The first attempt is
  usually blocked silently — grant TypeIT under System Settings → Privacy &
  Security → **Accessibility**, then try again. (In `npm run dev` the host is the
  terminal/`Tauri` dev binary; a packaged `.app` prompts as itself.)
- **Windows:** `SendInput` works without special permission, but it can't type
  into an app running **as administrator** unless TypeIT is elevated too.
- Show/hide TypeIT anytime with **Ctrl+Shift+Space**.

## 4d. Logs (for troubleshooting)

TypeIT writes a log file on startup and while running:

- **Windows:** `%APPDATA%\com.typeit.app\logs\typeit.log`
  (paste `%APPDATA%\com.typeit.app\logs` into Explorer's address bar)
- **macOS:** `~/Library/Logs/com.typeit.app/typeit.log`

It records whether the window was shown, whether the global shortcuts
registered (a clash is logged, not fatal), and each typing attempt. If the
window doesn't appear, this file says why.

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

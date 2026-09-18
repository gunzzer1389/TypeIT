# DEV_CONTEXT.md — TypeIT

Condensed working ledger. Newest entries on top.

---

## What TypeIT is (standing scope)

TypeIT is a **hands-free dictation helper**: a frameless, translucent,
always-on-top desktop widget you talk to when your hands are busy. You speak,
it captures and transcribes, you read/edit the text in place, then you drop it
into whatever field you're focused on (email, WhatsApp, a YouTube comment, a
chat box). It's built to catch ideas as they come and refine them without
losing anything.

**In scope / the feature set:**

- **Speech-to-text via Deepgram** — mic capture streamed to the Deepgram API,
  live transcript into the widget.
- **Review & edit in place** — the transcript lands in an editable field so you
  can fix wording before it goes anywhere.
- **Type into the focused window** — you place the cursor in the target app,
  trigger TypeIT, and it types the reviewed text there (standard dictation
  keystroke delivery to the active/foreground window).
- **Notes history** — keep roughly the last ~20 captures so nothing is lost
  mid-brainstorm; revisit/reuse past dictations.
- **The neutral shell** — transparent frameless window, opacity control,
  always-on-top, and a hotkey to show/hide TypeIT's *own* window.

**Explicitly out of scope (will not be built):** hiding the app from Task
Manager / process listings, evading screen capture or app switchers for
concealment, and process/binary-name masquerading. TypeIT is a visible,
ordinary desktop tool.

---

## Roadmap / pending

- [~] **Deepgram streaming** — mic → live transcript into an editable box.
      *Done:* `app/src/dictation.js` (getUserMedia → 16 kHz linear16 PCM →
      Deepgram realtime WS via `["token", key]` subprotocol; interim/final
      handling, KeepAlive, clean teardown), transcript/interim/mic/clear/copy UI
      in `index.html`, API key stored in `localStorage`, macOS mic Info.plist.
      *Left:* verify on a native build (WSL can't run it); move the key to an OS
      keychain later; consider AudioWorklet over the deprecated ScriptProcessor.
- [~] **Type-into-focused-window** — deliver reviewed text as keystrokes to the
      foreground app. *Done:* `enigo` in Cargo; `type_text` (hides window →
      250 ms → types) and `set_pending_text` commands + `Ctrl+Shift+Enter`
      global hotkey (`lib.rs`); "Type it" button + hotkey sync (`dictation.js`).
      *Left:* native verify; macOS needs Accessibility permission granted;
      can't type into elevated apps on Windows unless TypeIT is elevated.
- [ ] **Notes history** — persist the last ~20 captures locally; UI to browse
      and re-send them.
- [x] **Typing speed range (wpm)** — min–max fields; backend jitters each
      char's pace within the band (averages near midpoint). Stored in
      `localStorage`, synced to Rust for the hotkey path.
- [x] **Tray-only / no taskbar** — background utility: `skipTaskbar` (Win/Linux)
      + macOS Accessory activation policy. Reached via tray + hotkey. Not
      concealment — still a normal, visible process.
- [ ] User builds the shell on Windows and/or macOS (`app/BUILD.md`) and reports
      how the transparent widget looks/behaves natively.
- [ ] Iterate visual polish based on native rendering (blur, radius, sizing).
- [ ] Optional shell niceties: remember window position, tray entry,
      configurable opacity hotkey.

---

## 2026-09-18 — Stop / no-double-type / resume from where it left off

- **Reported:** hitting Type it again mid-run started a second pass → interleaved
  characters, scrambled output; had to wait it out and kill via Task Manager.
- **Fixes (`lib.rs`):** `static TYPING` guard (a second call returns
  `Err("Already typing")` instead of running concurrently — the real cause);
  `static CANCEL` checked each char so typing can be stopped; `type_string_range`
  returns chars typed (`usize`); `stop_typing` command + global **Stop hotkey
  Ctrl+Shift+Backspace** (Stop must be global since the button path hides the
  window). `type_text` returns the typed count.
- **Frontend:** Type it disables itself + shows **Stop** while running; on a
  partial (stopped) result it puts the *remaining* text back in the box (sliced
  by `Array.from` to match Rust `chars()`) so the next Type it resumes. Stop
  button calls `stop_typing`.
- **Taskbar:** also call `win.set_skip_taskbar(true)` in setup (belt-and-braces
  with the config flag), since the user still saw the icon (was on a pre-0.1.2
  build).
- **Deferred:** auto-stop when focus leaves the target window + resume-on-return
  — needs OS foreground-window watching (Win32/AppKit FFI) that can't be tested
  from WSL; doing it next so a compile risk there can't block this working core.

## 2026-09-18 — Speed as a range + tray-only (no taskbar)

- **Typing speed is now a min–max range** (defaults 180–220). `lib.rs`:
  `SpeedRange(Mutex<(u32,u32)>)` state; `type_string_range()` picks each char's
  wpm uniformly in [min,max] via a tiny dependency-free xorshift `Rng` (seeded
  from the clock), so pace varies naturally and averages near the midpoint.
  Commands `set_speed_range(min,max)` and `type_text(text,min,max)`; hotkey path
  reads the range from state. Frontend: two number inputs (`wpmMin`/`wpmMax`),
  ordered + clamped, persisted to `localStorage`, synced via `set_speed_range`
  (camelCase `minWpm`/`maxWpm` → snake_case params, Tauri's default mapping).
- **Runs as a background tray app, off the taskbar:** `skipTaskbar: true` in
  `tauri.conf.json` (Windows/Linux) and `ActivationPolicy::Accessory` on macOS
  (no Dock icon). Reached via the tray icon + `Ctrl+Shift+Space`. This is normal
  tray-utility behavior, NOT the out-of-scope concealment (still a visible
  process in Task Manager/Activity Monitor).

## 2026-09-18 — Clean upgrades: tray Quit, single-instance, version bump

- **Reported:** running the new installer still launched the *old* app.
- **Cause:** window "close" only *hides* (no way to quit), so the old instance
  kept running in the background and locked its files, blocking the installer
  from replacing them.
- **Fixes:** system **tray** icon (Show/Hide + **Quit TypeIT** → `app.exit(0)`)
  so the app can actually be closed — needs `tauri` feature `tray-icon`;
  **`tauri-plugin-single-instance`** (registered first) focuses the existing
  window instead of opening a duplicate; **version 0.1.0 → 0.1.1** across
  `tauri.conf.json` / `package.json` / `Cargo.toml` so installers treat it as an
  upgrade. Documented Quit/Update steps in `BUILD.md`.
- **User cleanup for the stuck state:** end `TypeIT.exe` in Task Manager,
  uninstall old via Settings → Apps, then install the new build. Bump the
  version on each future release.

## 2026-09-18 — Startup robustness + file logging (diagnostics)

- **Reported:** first native run — "don't see it anywhere" (window absent),
  can't type, can't find the Deepgram link.
- **Root-cause suspects:** (1) `setup` used `?` on shortcut registration, so a
  hotkey clash (Ctrl+Shift+Space / +Enter already owned) would abort startup and
  show no window; (2) app installed but not launched; (3) transparent window not
  compositing. The Deepgram link is in the in-flow settings panel *under* the
  widget (shown by default; ~380 px down) — reachable once the window shows.
- **Fixes/diagnostics shipped:** shortcut registration is now non-fatal (logged,
  app continues); `setup` explicitly `show()/unminimize()/set_focus()` the main
  window and logs `visible=`; panic hook logs to the log file; added
  `tauri-plugin-log` (+ `log`) writing to the app log dir and stdout; typing
  paths log enigo init/typing errors. Log paths documented in `BUILD.md`.
- **Next:** user runs this build, sends `typeit.log`; if it shows
  `visible=Ok(true)` but still nothing on screen → transparency/compositor, flip
  `transparent`/add windowEffects.

## 2026-09-18 — Deepgram key moved into Settings + "Get a key" link

- **Key management now lives in the gear/settings popover** (renamed aria-label
  to "Settings") as a "Deepgram API key" section: password input, Save, a status
  line ("Key saved ✓" / "No key saved"), and a **Get a Deepgram API key** button
  → opens `https://console.deepgram.com/` (sign-in / keys). Removed the one-shot top key bar
  (it vanished after first save, leaving no way to change a bad key).
- Pressing **Dictate** with no key now opens the settings panel and focuses the
  field instead of a dead-end error.
- **Opening external URLs:** added `tauri-plugin-opener` + a custom `open_url`
  command (custom commands aren't ACL-gated, so no capability entry needed);
  frontend `openExternal()` calls it, falling back to `window.open` in the
  browser preview.

## 2026-09-17 — Typing speed (words per minute)

- **`lib.rs`** — typing is now paced: `type_string_at(text, wpm)` types one char
  at a time, sleeping `12_000 / wpm` ms between chars (5-chars-per-word
  convention; wpm clamped 10–1000, default 240). New `Wpm(Mutex<u32>)` state +
  `set_wpm` command; `type_text` takes a `wpm` arg; the hotkey path reads wpm
  from state and now types on its own `std::thread` so paced typing never blocks
  the app.
- **`index.html` / `dictation.js`** — a "Typing speed" row: number input
  (10–1000, step 10) with −/+ steppers, persisted to `localStorage` and pushed
  to Rust on change. `Type it` passes the current wpm.
- Higher wpm ≈ faster/less reliable in some targets; lower is steadier. Default
  240 wpm ≈ 50 ms/char.

## 2026-09-17 — Type-into-focused-window

- **`Cargo.toml`** — added `enigo = "0.2"` for cross-platform keystroke
  synthesis (Windows SendInput, macOS CGEvent, Linux X11/Wayland).
- **`lib.rs`** — new `PendingText(Mutex<String>)` app state; commands
  `set_pending_text` (frontend keeps it synced, debounced) and `type_text`
  (async: hides our window so the prior app refocuses → 250 ms on a blocking
  thread → types). Registered a second global shortcut **Ctrl+Shift+Enter**
  that types the pending text into whatever you've focused (no hide, since the
  target is already frontmost). Scope note rewritten: delivers to the
  *foreground* window only; no window targeting/enumeration, no capture, no
  process hiding.
- **`index.html` / `dictation.js`** — "Type it" primary button (Copy demoted to
  a ghost button); button hides+types via `type_text`, and the transcript is
  pushed to Rust on input so the hotkey path works when the webview is blurred.
  Disclaimer updated with the two type paths.
- **macOS caveat:** `enigo` needs **Accessibility** permission or typing is
  silently blocked; documented in `BUILD.md`. **Windows caveat:** can't type
  into elevated apps unless TypeIT is elevated too.
- **Not yet verified natively.**

## 2026-09-17 — Deepgram dictation (first feature) wired in

- **Added `app/src/dictation.js`** — the mic→STT pipeline: `getUserMedia`
  (mono, echo/noise-cancel) → `AudioContext` + `ScriptProcessorNode` → Float32
  downsampled to 16 kHz `linear16` PCM → Deepgram realtime WebSocket
  (`wss://api.deepgram.com/v1/listen`, model `nova-2`, `smart_format`,
  `punctuate`, `interim_results`). Browser WS can't set headers, so auth uses
  the `["token", <key>]` subprotocol. Interim words show in the accent line;
  finals append to the editable textarea. KeepAlive every 8s; `CloseStream` +
  full graph teardown on stop.
- **`index.html`** — replaced the static mockup compose area with a real
  `<textarea>` transcript + interim line, a mic toggle (red while listening),
  status readout, Clear, and Copy (clipboard; interim stand-in until keystroke
  delivery lands). API-key bar shows until a key is saved to `localStorage`.
  Refreshed the meta description and on-page disclaimer to describe the tool
  honestly (records mic only while dictating; no screen capture / no hiding).
- **Native mic:** added `src-tauri/Info.plist` with
  `NSMicrophoneUsageDescription` (WKWebView needs it or macOS denies the mic);
  documented Windows/macOS mic prompts + Deepgram key in `BUILD.md`.
- **No new Tauri/Rust permissions** needed — capture, WS, and clipboard all run
  in the webview (CSP is `null`).
- **Not yet verified natively** (no GUI/mic in the WSL dev box).

## 2026-09-17 — Pivot to dictation tool

- **New direction:** TypeIT becomes a personal hands-free dictation helper
  (speak → transcribe → review → type into the focused app), with a local notes
  history. Deepgram chosen for speech-to-text.
- Concealment/anti-detection ideas from an earlier draft were dropped; TypeIT is
  a visible desktop tool.

## 2026-09-17 18:04 CDT — Desktop shell scaffolded (Tauri v2)

- **Decision:** Package the UI as a **Tauri v2** app (`app/`), chosen over
  Electron for footprint and over PyQt for distributable binaries.
- **Added** `app/`:
  - `src/index.html` — the UI (from the design study, adapted for a transparent
    native window: transparent body, `data-tauri-drag-region` titlebar, native
    window controls via `window.__TAURI__`).
  - `src-tauri/` — Rust core: `lib.rs` (window + one global show/hide shortcut,
    `set_always_on_top` command), `main.rs`, `Cargo.toml`, `tauri.conf.json`
    (frameless, transparent, alwaysOnTop, `macOSPrivateApi`), `capabilities/`.
  - `BUILD.md` — per-OS prerequisites and run/build steps.
  - `app-icon.png` — placeholder icon (regenerate set via `npm run icons`).
- **Constraint recorded:** WSL cannot cross-compile Windows/macOS GUI binaries;
  builds must run on the target OS. User is on WSL/Ubuntu → will test on Windows
  and/or a Mac.
- **UI decisions carried in:** settings live in an **in-flow panel under the
  widget** (not an overlay), toggled by the gear icon; titlebar hardened so the
  eye/gear tools never clip.
- **State:** Scaffold complete and committed on `dev`. Not yet compiled on a
  target OS (no Rust toolchain in the WSL dev box).

## 2026-09-17 (earlier) — Design study

- Built a look-and-feel mockup of the translucent widget. Published as a Claude
  artifact and served locally for review.
- Palette: deep indigo/teal ground, periwinkle accent (#8b8dff); type: IBM Plex
  Sans + IBM Plex Mono. Single dark "glass" world.

## 2026-09-17 (earlier) — Repo init

- Initialized git repo, pushed to private GitHub `gunzzer1389/TypeIT`.
- Branches: `main` (stable), `dev` (active work).

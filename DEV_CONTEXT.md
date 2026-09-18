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
- [ ] **Type-into-focused-window** — deliver the reviewed text as keystrokes to
      the foreground app; global hotkey to trigger.
- [ ] **Notes history** — persist the last ~20 captures locally; UI to browse
      and re-send them.
- [ ] User builds the shell on Windows and/or macOS (`app/BUILD.md`) and reports
      how the transparent widget looks/behaves natively.
- [ ] Iterate visual polish based on native rendering (blur, radius, sizing).
- [ ] Optional shell niceties: remember window position, tray entry,
      configurable opacity hotkey.

---

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

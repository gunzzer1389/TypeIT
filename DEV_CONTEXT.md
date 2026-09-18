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

- [ ] **Deepgram streaming** — mic → live transcript into an editable box in the
      widget (first build step). Prompt for and locally store the API key
      (never hardcoded).
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

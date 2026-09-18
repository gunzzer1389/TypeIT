/* ------------------------------------------------------------------------- *
 * TypeIT — dictation
 *
 * Mic capture -> Deepgram streaming speech-to-text -> editable transcript.
 *
 * Flow: press Dictate, we open the microphone and stream 16 kHz linear16 PCM
 * to Deepgram's realtime WebSocket. Interim words show in the accent-coloured
 * line under the box; when Deepgram marks a chunk final, it is appended to the
 * editable textarea, where you can fix wording before sending.
 *
 * Scope: this records the microphone ONLY while dictation is on, and only to
 * transcribe your own speech. It does not capture the screen or touch other
 * applications. The Deepgram API key is stored locally (localStorage) on this
 * machine and sent only to Deepgram.
 *
 * Next step (not built here): deliver the reviewed text as keystrokes into the
 * window you have focused. For now, Copy puts it on the clipboard.
 * ------------------------------------------------------------------------- */
(function () {
  "use strict";

  var KEY_STORE = "typeit.deepgram.key";
  var TARGET_RATE = 16000; // what we tell Deepgram we're sending
  // Console root: signs existing users in and lands on their keys; new users can
  // sign up from the same page.
  var DEEPGRAM_CONSOLE = "https://console.deepgram.com/";

  // --- Tauri bridge (undefined in a plain browser preview) --------------
  var tauri  = window.__TAURI__;
  var invoke = tauri && tauri.core && tauri.core.invoke;

  // --- Elements ---------------------------------------------------------
  var transcriptEl = document.getElementById("transcript");
  var interimEl    = document.getElementById("interim");
  var micBtn       = document.getElementById("micBtn");
  var micLabel     = document.getElementById("micLabel");
  var statusEl     = document.getElementById("dictStatus");
  var clearBtn     = document.getElementById("clearBtn");
  var copyBtn      = document.getElementById("copyBtn");
  var keyInput     = document.getElementById("keyInput");
  var keySave      = document.getElementById("keySave");
  var keyStatus    = document.getElementById("keyStatus");
  var getKeyBtn    = document.getElementById("getKeyBtn");
  var settingsPanel = document.getElementById("seethrough");
  var gearBtn      = document.getElementById("gearBtn");

  // Open an external URL in the default browser (via the Rust opener command),
  // falling back to window.open for the browser preview.
  async function openExternal(url) {
    if (invoke) {
      try { await invoke("open_url", { url: url }); return; }
      catch (e) { console.error("open_url failed:", e); }
    }
    try { window.open(url, "_blank", "noopener"); } catch (_) {}
  }

  // Reveal the settings panel (where the key field lives).
  function openSettings() {
    if (settingsPanel) settingsPanel.setAttribute("aria-hidden", "false");
    if (gearBtn) gearBtn.setAttribute("aria-expanded", "true");
  }

  // --- Live capture state (null when idle) ------------------------------
  var stream = null;      // MediaStream from getUserMedia
  var audioCtx = null;    // AudioContext
  var sourceNode = null;  // MediaStreamAudioSourceNode
  var procNode = null;    // ScriptProcessorNode (PCM pump)
  var ws = null;          // Deepgram WebSocket
  var keepAlive = null;   // interval id for KeepAlive pings
  var listening = false;

  // ---------------------------------------------------------------------
  // Key handling
  // ---------------------------------------------------------------------
  function getKey() {
    try { return localStorage.getItem(KEY_STORE) || ""; }
    catch (_) { return ""; }
  }
  function setKey(v) {
    try { localStorage.setItem(KEY_STORE, v); } catch (_) {}
  }
  function refreshKeyUI() {
    if (!keyStatus) return;
    var has = !!getKey();
    keyStatus.textContent = has ? "Key saved ✓" : "No key saved";
    keyStatus.classList.toggle("ok", has);
  }

  if (keySave) {
    keySave.addEventListener("click", function () {
      var v = (keyInput.value || "").trim();
      if (!v) { setStatus("Enter a key first", "error"); return; }
      setKey(v);
      keyInput.value = "";
      refreshKeyUI();
      setStatus("Key saved", "live");
    });
  }
  if (keyInput) {
    keyInput.addEventListener("keydown", function (e) {
      if (e.key === "Enter") keySave.click();
    });
  }
  if (getKeyBtn) {
    getKeyBtn.addEventListener("click", function () { openExternal(DEEPGRAM_CONSOLE); });
  }

  // ---------------------------------------------------------------------
  // Status helper
  // ---------------------------------------------------------------------
  function setStatus(text, kind) {
    if (!statusEl) return;
    statusEl.textContent = text;
    statusEl.classList.remove("error", "live");
    if (kind) statusEl.classList.add(kind);
  }

  // ---------------------------------------------------------------------
  // Transcript writing
  // ---------------------------------------------------------------------
  function appendFinal(text) {
    if (!text) return;
    var cur = transcriptEl.value;
    // Join with a space unless the box is empty or already ends with whitespace.
    if (cur && !/\s$/.test(cur)) cur += " ";
    transcriptEl.value = cur + text;
    transcriptEl.scrollTop = transcriptEl.scrollHeight;
  }

  // ---------------------------------------------------------------------
  // Deepgram message handling
  // ---------------------------------------------------------------------
  function onDeepgramMessage(evt) {
    var msg;
    try { msg = JSON.parse(evt.data); } catch (_) { return; }
    if (msg.type && msg.type !== "Results") return; // Metadata / other frames
    var alt = msg.channel &&
              msg.channel.alternatives &&
              msg.channel.alternatives[0];
    if (!alt) return;
    var text = (alt.transcript || "").trim();
    if (!text) return;

    if (msg.is_final) {
      appendFinal(text);
      interimEl.textContent = "";
    } else {
      interimEl.textContent = text;
    }
  }

  // ---------------------------------------------------------------------
  // Audio -> PCM. WebView getUserMedia hands us float samples at the
  // context's rate; convert to 16-bit little-endian and downsample to
  // TARGET_RATE by simple decimation (good enough for speech).
  // ---------------------------------------------------------------------
  function floatToPCM16(input, inRate) {
    var ratio = inRate / TARGET_RATE;
    var outLen = Math.floor(input.length / ratio);
    var buf = new ArrayBuffer(outLen * 2);
    var view = new DataView(buf);
    for (var i = 0; i < outLen; i++) {
      var s = input[Math.floor(i * ratio)];
      s = Math.max(-1, Math.min(1, s));
      view.setInt16(i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    }
    return buf;
  }

  // ---------------------------------------------------------------------
  // Start / stop
  // ---------------------------------------------------------------------
  async function start() {
    var key = getKey();
    if (!key) {
      openSettings();
      keyInput && keyInput.focus();
      setStatus("Add your Deepgram key", "error");
      return;
    }
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      setStatus("No microphone API", "error");
      return;
    }

    setStatus("Connecting…");
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true }
      });
    } catch (err) {
      setStatus("Mic blocked", "error");
      console.error("getUserMedia failed:", err);
      return;
    }

    var params = new URLSearchParams({
      model: "nova-2",
      encoding: "linear16",
      sample_rate: String(TARGET_RATE),
      channels: "1",
      interim_results: "true",
      smart_format: "true",
      punctuate: "true"
    });
    var url = "wss://api.deepgram.com/v1/listen?" + params.toString();

    // Browser WebSockets can't set an Authorization header, so Deepgram
    // accepts the key via the Sec-WebSocket-Protocol pair ["token", <key>].
    try {
      ws = new WebSocket(url, ["token", key]);
    } catch (err) {
      cleanup();
      setStatus("Connection failed", "error");
      console.error("WebSocket construct failed:", err);
      return;
    }
    ws.binaryType = "arraybuffer";

    ws.onopen = function () {
      // Audio graph: source -> processor. The processor fires per buffer;
      // we ship each buffer as PCM. (ScriptProcessorNode is deprecated but
      // is the most portable option across WebView2/WKWebView today; we can
      // move to an AudioWorklet later.)
      audioCtx = new (window.AudioContext || window.webkitAudioContext)();
      sourceNode = audioCtx.createMediaStreamSource(stream);
      procNode = audioCtx.createScriptProcessor(4096, 1, 1);

      procNode.onaudioprocess = function (e) {
        if (!ws || ws.readyState !== WebSocket.OPEN) return;
        var pcm = floatToPCM16(e.inputBuffer.getChannelData(0), audioCtx.sampleRate);
        ws.send(pcm);
      };

      sourceNode.connect(procNode);
      procNode.connect(audioCtx.destination); // required for the node to run

      // Deepgram drops the socket after ~10s with no audio; keep it warm.
      keepAlive = setInterval(function () {
        if (ws && ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: "KeepAlive" }));
        }
      }, 8000);

      listening = true;
      micBtn.setAttribute("aria-pressed", "true");
      micLabel.textContent = "Stop";
      setStatus("Listening", "live");
    };

    ws.onmessage = onDeepgramMessage;

    ws.onerror = function (err) {
      console.error("Deepgram socket error:", err);
      setStatus("Stream error", "error");
    };

    ws.onclose = function (e) {
      // 1008 / 4001-ish closes usually mean auth trouble.
      if (listening && e && e.code !== 1000) {
        setStatus(e.code === 1008 ? "Key rejected" : "Disconnected", "error");
      }
      stop();
    };
  }

  function stop() {
    // Ask Deepgram to flush any pending words, then tear down the graph.
    if (ws && ws.readyState === WebSocket.OPEN) {
      try { ws.send(JSON.stringify({ type: "CloseStream" })); } catch (_) {}
    }
    cleanup();
    listening = false;
    micBtn.setAttribute("aria-pressed", "false");
    micLabel.textContent = "Dictate";
    interimEl.textContent = "";
    if (!statusEl.classList.contains("error")) setStatus("Idle");
  }

  function cleanup() {
    if (keepAlive) { clearInterval(keepAlive); keepAlive = null; }
    if (procNode) { try { procNode.disconnect(); } catch (_) {} procNode.onaudioprocess = null; procNode = null; }
    if (sourceNode) { try { sourceNode.disconnect(); } catch (_) {} sourceNode = null; }
    if (audioCtx) { try { audioCtx.close(); } catch (_) {} audioCtx = null; }
    if (stream) { stream.getTracks().forEach(function (t) { t.stop(); }); stream = null; }
    if (ws) {
      ws.onopen = ws.onmessage = ws.onerror = ws.onclose = null;
      try { if (ws.readyState <= WebSocket.OPEN) ws.close(); } catch (_) {}
      ws = null;
    }
  }

  // ---------------------------------------------------------------------
  // Controls
  // ---------------------------------------------------------------------
  if (micBtn) {
    micBtn.addEventListener("click", function () {
      if (listening) stop(); else start();
    });
  }

  if (clearBtn) {
    clearBtn.addEventListener("click", function () {
      transcriptEl.value = "";
      interimEl.textContent = "";
      transcriptEl.focus();
    });
  }

  if (copyBtn) {
    copyBtn.addEventListener("click", async function () {
      var text = transcriptEl.value.trim();
      if (!text) { setStatus("Nothing to copy", "error"); return; }
      try {
        await navigator.clipboard.writeText(text);
        setStatus("Copied", "live");
      } catch (_) {
        // Fallback for webviews without the async clipboard API.
        transcriptEl.select();
        try { document.execCommand("copy"); setStatus("Copied", "live"); }
        catch (e2) { setStatus("Copy failed", "error"); }
      }
    });
  }

  // ---------------------------------------------------------------------
  // Type into the focused window (Tauri only).
  //
  //   * Type it button -> `type_text`: hides our window so the app you had
  //     focused regains focus, then types the transcript there.
  //   * Ctrl+Shift+Enter (global) -> types the "pending" text into whatever
  //     you've clicked into. We keep that pending text in sync below.
  //
  // In a plain browser (no __TAURI__) the button just shows a hint, so the
  // same file still previews over http://localhost.
  // ---------------------------------------------------------------------
  var typeBtn = document.getElementById("typeBtn");

  // --- Typing speed range (words per minute) ---------------------------
  // The actual per-character pace is jittered between min and max in the Rust
  // backend, averaging near the middle for a natural, human feel.
  var WPM_MIN_STORE = "typeit.wpm.min", WPM_MAX_STORE = "typeit.wpm.max";
  var WPM_LO = 10, WPM_HI = 1000, DEF_MIN = 180, DEF_MAX = 220;
  var wpmMin = document.getElementById("wpmMin");
  var wpmMax = document.getElementById("wpmMax");

  function clampWpm(n, fallback) {
    n = Math.round(Number(n));
    if (!isFinite(n)) n = fallback;
    return Math.max(WPM_LO, Math.min(WPM_HI, n));
  }
  // Returns the ordered [min, max] currently in the fields.
  function currentRange() {
    var lo = clampWpm(wpmMin ? wpmMin.value : DEF_MIN, DEF_MIN);
    var hi = clampWpm(wpmMax ? wpmMax.value : DEF_MAX, DEF_MAX);
    return lo <= hi ? [lo, hi] : [hi, lo];
  }

  function applyRange(opts) {
    var r = currentRange();
    if (wpmMin) wpmMin.value = r[0];
    if (wpmMax) wpmMax.value = r[1];
    try {
      localStorage.setItem(WPM_MIN_STORE, String(r[0]));
      localStorage.setItem(WPM_MAX_STORE, String(r[1]));
    } catch (_) {}
    if (invoke) invoke("set_speed_range", { minWpm: r[0], maxWpm: r[1] }).catch(function () {});
    if (opts && opts.announce) setStatus(r[0] + "–" + r[1] + " wpm");
  }

  if (wpmMin && wpmMax) {
    // Restore saved range (falling back to the markup defaults).
    try {
      var sMin = localStorage.getItem(WPM_MIN_STORE);
      var sMax = localStorage.getItem(WPM_MAX_STORE);
      if (sMin != null) wpmMin.value = sMin;
      if (sMax != null) wpmMax.value = sMax;
    } catch (_) {}
    applyRange();
    wpmMin.addEventListener("change", function () { applyRange({ announce: true }); });
    wpmMax.addEventListener("change", function () { applyRange({ announce: true }); });
  }

  // Keep the Rust side's copy of the transcript current (debounced), so the
  // global hotkey has something to type when the webview isn't focused.
  if (invoke) {
    var syncTimer = null;
    transcriptEl.addEventListener("input", function () {
      clearTimeout(syncTimer);
      syncTimer = setTimeout(function () {
        invoke("set_pending_text", { text: transcriptEl.value }).catch(function () {});
      }, 200);
    });
  }

  var stopBtn = document.getElementById("stopBtn");

  // While typing: disable Type it (prevents the double-trigger that scrambles
  // output) and reveal Stop.
  function setTypingUI(on) {
    if (typeBtn) typeBtn.disabled = on;
    if (stopBtn) stopBtn.hidden = !on;
  }

  if (stopBtn) {
    stopBtn.addEventListener("click", function () {
      if (invoke) invoke("stop_typing").catch(function () {});
    });
  }

  if (typeBtn) {
    typeBtn.addEventListener("click", async function () {
      if (typeBtn.disabled) return;
      var text = transcriptEl.value.trim();
      if (!text) { setStatus("Nothing to type", "error"); return; }
      if (!invoke) { setStatus("Type-it needs the desktop app", "error"); return; }
      if (listening) stop(); // don't type our own mic stream
      var r = currentRange();
      setTypingUI(true);
      setStatus("Typing…");
      try {
        await invoke("set_pending_text", { text: text });
        var typed = await invoke("type_text", { text: text, minWpm: r[0], maxWpm: r[1] });
        // Rust returns how many characters it actually typed. If fewer than the
        // whole text, it was stopped — leave the remaining text in the box so
        // the next Type it resumes from where it left off.
        var cps = Array.from(text);
        if (typeof typed === "number" && typed < cps.length) {
          var remaining = cps.slice(typed).join("");
          transcriptEl.value = remaining;
          invoke("set_pending_text", { text: remaining }).catch(function () {});
          setStatus("Stopped · " + (cps.length - typed) + " left", "error");
        } else {
          setStatus("Typed", "live");
        }
      } catch (err) {
        var msg = String(err);
        setStatus(msg.indexOf("Already") >= 0 ? "Already typing" : "Type failed", "error");
        console.error("type_text failed:", err);
      } finally {
        setTypingUI(false);
      }
    });
  }

  // Stop cleanly if the window goes away mid-dictation.
  window.addEventListener("beforeunload", cleanup);

  refreshKeyUI();
})();

# PRD: Offline Meeting Transcriber (working title)

Status: Draft v0.2
Target platforms: macOS and Windows (mobile is out of scope for now)
Audience: Claude Code. Build from this document. Where a decision is marked "Decision", follow it. Where marked "Open", pick the simplest option and note the choice in `DECISIONS.md`.

---

## 1. Summary

A desktop app that transcribes meetings in real time from the computer's microphone, and transcribes uploaded audio or video files. All transcription runs locally using the open-source Whisper model. No audio or text leaves the machine. The app is free to run because there are no per-minute API costs.

## 2. Goals

1. Capture every word spoken. No dropped audio, even when transcription runs behind real time.
2. Free to operate: no cloud transcription, no account, no subscription.
3. Private by default: works fully offline after the first model download.
4. Good accuracy through user-selectable Whisper model sizes.
5. Simple enough that a non-technical user can record a meeting in under 30 seconds from first launch.

## 3. Non-goals (v1)

- Mobile apps
- Speaker identification (diarization)
- AI summaries and action items
- Capturing system audio from Zoom/Meet/Teams (planned for v1.1, see section 12)
- Accounts, sync, or cloud storage
- Real-time translation

## 4. Users and primary scenarios

- **In-room meeting:** laptop on the table, user presses Record, reads the live transcript, saves it afterward.
- **Recorded file:** user drags in an mp3, wav, m4a, or mp4 and gets a transcript.
- **Re-run for accuracy:** user finishes a meeting transcribed with a fast model, then re-runs it with a larger model for a cleaner final transcript.

## 5. Functional requirements

### FR-1 First-launch setup
- On first launch, show a model picker (see section 8) with size, expected speed, and expected accuracy for each option.
- Download the chosen model with a progress bar, pause/resume, and checksum verification. Store in the app data directory.
- Recommend a default based on detected hardware (RAM, GPU/Apple Silicon).
- Users can download, switch, and delete models later in Settings.
- "Import model file" for users who cannot download in-app: accept a local GGML file, verify its SHA256 against the pinned list, and install it.
- After the download, the app must work with no internet connection.

### FR-2 Live transcription
- Record button starts capture from the selected input device (device picker in the UI, default system microphone).
- **No-loss guarantee (Decision):** raw audio is written continuously to disk as 16 kHz mono 16-bit PCM WAV from the moment recording starts, independent of transcription. Do not use FLAC for the live recording; a FLAC stream cut off by a crash is hard to recover, while a WAV only needs its header repaired. The writer flushes and fsyncs at least once per second. The transcription pipeline reads from a queue of speech segments. If transcription falls behind, the queue grows; audio is never discarded.
- **Queue entries reference audio on disk (Decision):** each queued segment stores sample offsets (start, end) into the session's audio file, not the samples themselves. Memory use stays flat no matter how far behind transcription is. The last transcribed offset is persisted, so crash recovery resumes from there instead of starting over.
- Voice activity detection (Silero VAD) splits audio into speech segments. Each segment is sent to Whisper when it ends, or at a maximum segment length of about 25 seconds. When the maximum is reached, cut at the lowest-energy point in the last few seconds rather than at exactly 25 s, to avoid splitting words.
- Do not pass the previous segment's text to Whisper as the initial prompt. M1 testing showed this caused whole sentences to be dropped (see `DECISIONS.md`). The initial prompt is reserved for user-supplied vocabulary (names, jargon).
- Whisper always encodes a 30-second window, so very short segments waste most of the work. Pack consecutive speech segments into one buffer of up to about 25 seconds, separated by 1 second of silence, and map timestamps back to the recording (implemented in M1 for file transcription). For live mode, balance this against latency.
- Transcript text appears per segment, with timestamps. Lag of several seconds is acceptable.
- UI shows a status indicator: Recording, Transcribing, "N seconds behind", and Caught up.
- Stop button flushes the queue; the app finishes transcribing everything captured before marking the session complete.
- Pause and resume recording within the same session. **Decision:** paused time is not written to the audio file (the audio is contiguous), and transcript timestamps are relative to that audio, so click-to-seek always matches playback. Pause points are stored in the session so the UI can mark them.
- Prevent system sleep while recording (macOS power assertion; Windows `SetThreadExecutionState`). If the system sleeps anyway (for example, lid closed), treat it like a device disconnect: pause, then warn on wake.
- **Disk space:** check free space before and during recording (16 kHz mono 16-bit WAV is about 115 MB per hour). Warn when under about 1 GB free, and stop recording cleanly before the disk is full so the file stays valid.
- Handle device disconnects (for example, unplugged headset) by pausing, warning the user, and allowing a device switch without losing the session.

### FR-3 File upload transcription
- Drag-and-drop or file picker. Supported: mp3, wav, m4a, flac, ogg, mp4, mov, mkv (extract audio with ffmpeg). FLAC is fine here and for archived audio; only the live recording file must be WAV.
- Runs as a background job with progress and an estimated time remaining.
- Multiple files can be queued; process one at a time.
- Long files (3+ hours) must not exhaust memory: process in chunks.

### FR-4 Re-transcribe
- From any saved session, the user can re-run transcription with a different installed model, using the saved audio.
- Keep the previous version until the user deletes it. Show versions as tabs or a dropdown.

### FR-5 Transcript view and editing
- Scrolling transcript with timestamps; click a line to jump audio playback to that point.
- Inline text editing, with edits saved to the session.
- Audio player with play/pause, seek, and playback speed (0.75x to 2x).
- Search within a transcript.

### FR-6 Library
- List of all sessions: title (editable), date, duration, model used, source (live or file).
- Full-text search across all transcripts (SQLite FTS5). Search covers the edited text where an edit exists, and only each session's active transcript version, so results are neither stale nor duplicated.
- Delete a session (removes audio and text, with confirmation).

### FR-7 Export
- Plain text, Markdown, SRT, VTT, JSON (with timestamps). DOCX is deferred to after v1 (Decision).
- Copy-all-to-clipboard.
- Export includes the session title, date, and model name in a header (optional toggle).

### FR-8 Language
- Auto-detect by default, with a manual language selector.
- Offer English-only model variants (`.en`) for tiny, base, and small when the user selects English; they are generally more accurate for English at the same size.

### FR-9 Privacy and consent
- No network calls except model download (and an optional update check, off by default).
- No telemetry in v1.
- Show a one-time reminder on first recording: recording laws vary and participants should consent. Include a short, dismissible banner during recording (setting to disable after the first time).

### FR-10 Settings
- Input device, model management, default language, transcript font size, storage location, auto-delete audio after N days (off by default), keep audio toggle.

## 6. Non-functional requirements

- **Reliability:** an app crash or force-quit during recording must not lose audio already written to disk. On next launch, offer to recover and transcribe the interrupted session.
- **Performance targets (reference machines):**
  - Apple Silicon M1 or newer, 8 GB RAM: `small` model transcribes live audio at least at real-time speed.
  - Windows laptop, 16 GB RAM, no dedicated GPU: `base` runs at least at real-time speed; `small` may run behind real time, and the UI must communicate this.
- **Resource use:** idle memory under 300 MB with no model loaded; app unloads the model after N minutes idle.
- **Battery:** warn when recording on battery with a large model selected.
- **Storage:** show disk space used per session and per model.
- **Accessibility:** keyboard navigable, screen-reader labels, adjustable text size.
- **Installers:** signed macOS .dmg (notarized) and Windows installer (.msi or .exe). Installer size under 150 MB (models are downloaded separately).
- **macOS permissions:** include `NSMicrophoneUsageDescription` in Info.plist and the `com.apple.security.device.audio-input` entitlement under the hardened runtime; without them, notarized builds cannot access the microphone. Handle a denied permission with a clear message and a link to System Settings.

## 7. Architecture and stack

**Decision: Tauri 2 (Rust backend) with a React + TypeScript frontend.**
Reason: small installers, native performance for audio, and good access to Rust bindings for whisper.cpp. If Tauri blocks progress on audio or packaging, fall back to Electron and record the reason in `DECISIONS.md`.

| Layer | Choice |
|---|---|
| Transcription engine | whisper.cpp via `whisper-rs` |
| Acceleration | Metal (Apple Silicon), Vulkan (Windows), CPU fallback. Auto-detect at startup. No CUDA in v1 (Decision): bundling the CUDA runtime alone would exceed the installer size limit. |
| Audio capture | `cpal` (cross-platform input) |
| Resampling | `rubato` or ffmpeg, to 16 kHz mono |
| VAD | Silero VAD via whisper.cpp's built-in support (`whisper-rs` standalone VAD context, GGML model `ggml-silero-v5.1.2.bin`, under 1 MB). No ONNX runtime. Confirmed in M1. |
| File decoding | ffmpeg, bundled as a sidecar binary. Use an LGPL build (no `--enable-gpl` / `--enable-nonfree`); only decoders are needed. |
| Storage | SQLite (with FTS5) for metadata and transcripts; audio as files on disk |
| Frontend | React, TypeScript, Vite, Tailwind |

**Pipeline:** `Mic -> ring buffer -> (1) disk writer (always) + (2) VAD -> segment queue (offsets into the WAV) -> Whisper worker process -> transcript events -> UI + SQLite`

Rules:
- The disk writer and VAD run on separate threads from each other and from the UI. Transcription slowness must never block capture.
- **Whisper inference runs in a separate sidecar process (Decision).** A thread is not enough: a native crash (segfault) inside whisper.cpp would kill the whole process, including capture. The main app supervises the worker, restarts it on crash, and re-queues the segment that was in flight.
- Use a single model instance, loaded on demand and reused across segments.

## 8. Models

Download from the official `ggerganov/whisper.cpp` Hugging Face repository (GGML format). Verify SHA256 after download against checksums pinned in the app (not fetched at runtime). Offer quantized variants (q5_0 or q8_0) to reduce size. Sizes are approximate.

| Option | Approx. size | Best for |
|---|---|---|
| Tiny | 75 MB | Weak hardware, fastest, lowest accuracy |
| Base | 140 MB | Default for low-end machines |
| Small | 470 MB | Recommended default for most laptops |
| Medium | 1.5 GB | Better accuracy, needs a strong machine |
| Large-v3-turbo | 1.6 GB | Near-large accuracy at much better speed; strong machines |
| Large-v3 | 3.1 GB | Highest accuracy, slow without a good GPU |

The UI should describe these in plain language ("Fast", "Balanced", "Accurate", "Most accurate"), with size shown beside each.

## 9. Data model (SQLite)

- `sessions(id, title, created_at, duration_sec, source_type[live|file], audio_path, language, status[recording|processing|complete|interrupted], transcribed_until_sample)`
- `session_pauses(session_id, at_sample, paused_at, resumed_at)`
- `transcript_versions(id, session_id, model_name, created_at, is_active)`
- `segments(id, version_id, start_ms, end_ms, text, edited_text)`
- `segments_fts` (FTS5 virtual table over `COALESCE(edited_text, text)`, containing only segments of active versions; kept in sync by triggers on `segments` and `transcript_versions.is_active`)
- `models(name, path, size_bytes, sha256, installed_at)`
- `settings(key, value)`

## 10. Screens

1. **Setup:** model picker and download.
2. **Home / Library:** session list, search, "Record" and "Import file" buttons.
3. **Live session:** large transcript area, status indicator, Record/Pause/Stop, elapsed time, input level meter.
4. **Session detail:** transcript with audio player, edit, search, export, re-transcribe.
5. **Settings.**

## 11. Milestones

1. **M1: Engine spike.** CLI or minimal window: load a Whisper model, transcribe a WAV file, print text with timestamps. Include a `bench` command that reports speed (real-time factor) per installed model. The owner runs it on the reference Mac (Metal and CPU) and Windows (CPU and Vulkan) machines; development builds may only be able to verify Linux CPU.
2. **M2: File transcription.** Import flow, ffmpeg decoding, chunked processing, SQLite storage, transcript view.
3. **M3: Live mode.** Mic capture, continuous disk writing, VAD, segment queue, live transcript UI, no-loss behavior, crash recovery.
4. **M4: Model management.** First-launch picker, downloads with resume and checksum, switch and delete models.
5. **M5: Library, search, export, re-transcribe.**
6. **M6: Polish and packaging.** Settings, signing, notarization, installers, performance tuning.

## 12. Later (not v1)

- **v1.1:** System audio capture so users can transcribe Zoom, Meet, and Teams calls (WASAPI loopback on Windows; ScreenCaptureKit on macOS 13+). Speaker diarization (sherpa-onnx).
- **v1.2:** Optional summaries and action items (bring-your-own API key or a local LLM).
- **Later:** mobile apps (iOS with WhisperKit or whisper.cpp; Android with whisper.cpp).

## 13. Acceptance criteria (v1)

- A 60-minute live recording produces a transcript covering the full duration with no gaps, even when the transcription pipeline is deliberately throttled to run slower than real time.
- Force-quitting during recording and relaunching recovers the audio and offers to transcribe it.
- With the network disabled after setup, all features (record, import, search, export, re-transcribe) still work.
- Importing a 3-hour file completes without running out of memory on a 16 GB machine.
- Switching models and re-transcribing a saved session produces a second version without deleting the first.
- Signed installers run on a clean macOS and Windows 11 machine.

## 14. Risks

| Risk | Mitigation |
|---|---|
| Whisper hallucinates text on silence or noise | Use VAD to skip non-speech; enable suppression of repeated output; test with silent and noisy samples |
| Live lag grows on slow machines | Queue design (nothing dropped); clear "behind by N seconds" UI; hardware-based default model |
| Large installer or download failures | Models downloaded separately with resume and checksum |
| Poor accuracy on some accents | Let users pick larger models and re-transcribe; build an accent test set early (M1) |
| Code signing and notarization delays | Start Apple Developer and Windows signing setup during M1 |
| Recording consent and legal exposure | Consent reminder (FR-9); keep audio local |

## 15. Open questions

- Product name and branding.
- License of the final app (free, open source, or free closed source). Dependencies are permissive except ffmpeg (LGPL, run as a separate process), which does not constrain this choice.

Resolved in v0.2:
- No model bundled in the installer. Instead, Settings offers "Import model file" so users who cannot download in-app can copy a GGML model in manually (checksum still verified).
- DOCX export deferred to after v1.

## 16. Instructions for Claude Code

1. Read this whole document first. Create `DECISIONS.md` and log any choice you make where this document says "Open" or where you deviate.
2. Work milestone by milestone. Do not start M(n+1) until M(n) meets its goal and has basic tests.
3. After M1, stop and report: transcription speed per model on the current machine (and how to run `bench` on the reference machines), whether whisper-rs exposes built-in VAD, and any blockers with Tauri or whisper-rs.
4. Write unit tests for the segment queue and the no-loss guarantee; these are the core promise of the product. Include: throttled worker (slower than real time), worker process crash mid-segment, app crash with a WAV whose header is unfinalized, and disk-full shutdown.
5. Keep dependencies minimal and note the license of each one in `LICENSES.md`.
6. Ask before adding any network call that is not model download.

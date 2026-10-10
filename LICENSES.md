# Dependency licenses

Runtime dependencies shipped in the app. Build-only dependencies (bindgen, cmake, cc, etc.) are not distributed and are omitted.

| Dependency | Version | License | Used for |
|---|---|---|---|
| whisper.cpp / ggml (bundled in whisper-rs-sys) | as pinned by whisper-rs-sys 0.15.0 | MIT | Transcription and VAD inference |
| whisper-rs | 0.16.0 | Unlicense | Rust bindings for whisper.cpp |
| whisper-rs-sys | 0.15.0 | Unlicense | Raw bindings, builds whisper.cpp |
| hound | 3.5.1 | Apache-2.0 | WAV reading and writing |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | Error types |

## Models (downloaded at runtime, not bundled)

| Model | License | Source |
|---|---|---|
| Whisper GGML models (`ggml-*.bin`) | MIT (OpenAI Whisper weights) | huggingface.co/ggerganov/whisper.cpp |
| Silero VAD v5.1.2 GGML (`ggml-silero-v5.1.2.bin`) | MIT | huggingface.co/ggml-org/whisper-vad |

## Planned

| Dependency | License | Note |
|---|---|---|
| ffmpeg (sidecar binary, M2) | LGPL-2.1+ | Use an LGPL build (no `--enable-gpl`/`--enable-nonfree`). Run as a separate process; ship its license and a source offer. |
| Tauri 2 | MIT OR Apache-2.0 | App shell (M2+) |

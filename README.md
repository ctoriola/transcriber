# Offline Meeting Transcriber

Local, offline meeting transcription with Whisper. See [PRD.md](PRD.md) for the product spec and [DECISIONS.md](DECISIONS.md) for implementation decisions.

Status: **M1 (engine spike)**. A command-line tool that transcribes a WAV file and benchmarks models.

## Build

Requires Rust (stable), CMake and a C/C++ compiler (Xcode command-line tools on macOS, Visual Studio Build Tools on Windows).

```sh
cargo build --release                          # CPU only
cargo build --release --features metal         # macOS, Apple Silicon GPU
cargo build --release --features vulkan        # Windows/Linux GPU (needs the Vulkan SDK and VULKAN_SDK set)
```

## Get models

```sh
mkdir models && cd models
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
curl -LO https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin
```

## Use

Input must be a 16 kHz WAV. Convert anything with ffmpeg:

```sh
ffmpeg -i meeting.m4a -ar 16000 -ac 1 -c:a pcm_s16le meeting.wav
```

```sh
target/release/transcriber-cli transcribe models/ggml-base.en.bin meeting.wav --vad models/ggml-silero-v5.1.2.bin
target/release/transcriber-cli info   # shows which CPU/GPU backends are compiled in
```

Options: `--lang CODE` (default: auto-detect), `--threads N`, `--cpu` (disable GPU), `--vad PATH` (recommended: skips silence, which prevents made-up text).

## Benchmark on the reference machines (PRD section 6)

Use a few minutes of real meeting audio, converted as above:

```sh
target/release/transcriber-cli bench meeting.wav models/ggml-tiny.en.bin models/ggml-base.en.bin models/ggml-small.en.bin --vad models/ggml-silero-v5.1.2.bin
```

"x real time" above 1.0 means faster than real time; live mode needs at least 1.0.

- **Mac (M1+, 8 GB):** build with `--features metal`, then run once as is (Metal) and once with `--cpu`. Target: `small` at least 1.0.
- **Windows (16 GB, no dedicated GPU):** build CPU-only, run it; then build with `--features vulkan` and run again. Target: `base` at least 1.0.

## Test

```sh
cargo test --release
```

Tests that need models skip unless these are set (use absolute paths):

```sh
WHISPER_TEST_MODEL=$PWD/models/ggml-tiny.en.bin \
WHISPER_TEST_VAD=$PWD/models/ggml-silero-v5.1.2.bin \
WHISPER_TEST_AUDIO=/path/to/speech-16k.wav \
cargo test --release
```

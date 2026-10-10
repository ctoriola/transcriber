# Decisions

Choices made where the PRD said "Open", or where implementation deviates from it.

## M1: Engine spike (2026-10-10)

**Workspace layout.** Cargo workspace with `crates/engine` (library: audio loading, Whisper, VAD) and `crates/cli` (`transcriber-cli`). The Tauri app will be added as another crate that depends on `engine`. Tauri itself was not exercised in M1, which only needs a CLI.

**whisper-rs 0.16 (whisper.cpp bundled).** Cargo features `metal` and `vulkan` select the GPU backend at build time; the default build is CPU only. whisper-rs also has `cuda`, which we do not use (PRD: Vulkan on Windows).

**Built-in Silero VAD, run standalone.** whisper.cpp's `params.vad` flag only works in `whisper_full()`; whisper-rs calls `whisper_full_with_state()`, which silently ignores it (verified: VAD-enabled runs still hallucinated on silence). We run the standalone `WhisperVadContext` and feed Whisper the speech ourselves (`engine::vad`). No ONNX runtime is needed.

**VAD is required against hallucination.** On 10 s of digital silence, `tiny` and `base.en` without VAD output "you" (no-speech probability was as low as 2e-6, so it cannot be filtered on that). With VAD: no output. `tiny.en` happened to output nothing even without VAD.

**Pack speech into chunks of up to 25 s, with a 1 s gap.** Whisper always encodes a 30 s window. Sending each VAD span separately produced 18 calls for 117 s of audio and made `small` about 3x slower (91 s versus 33 s). Spans with a pause of 1 s or less stay one piece (real pause kept); pieces are packed into buffers of up to 25 s with silence between, and timestamps are mapped back to the original audio.

Gap size was chosen by measurement (117 s of audio with 5 instances each of three known sentences, counting sentences recovered):

| model | 200 ms | 500 ms | 1000 ms | 2000 ms |
|---|---|---|---|---|
| tiny.en | 15/15 | 15/15 | 15/15 | 15/15 |
| base.en | 14/15 | 15/15 | 15/15 | 15/15 |
| small.en | 14/15 | 15/15 | 15/15 | 15/15 |

Speed differed by under 10% between 200 ms and 1000 ms, so 1000 ms was chosen for margin.

**No prompt chaining (deviation from PRD v0.2).** The PRD (on my recommendation) said to pass the previous segment's text as the initial prompt. Measured on the same audio, prompt chaining lost sentences on every model (tiny.en 9/15, base.en 10/15, small.en 11/15) and was about 3x slower, because Whisper copied the prompt, failed its quality checks and retried at higher temperatures. Without chaining: 15/15 at 1000 ms gap. `initial_prompt` stays available for user-supplied vocabulary. Caveat: the test audio repeats the same clips, which makes copying more likely than in a real meeting, but the loss is severe enough not to risk.

**Greedy decoding (best_of 1).** Fastest option, used for both benchmarks. Beam search may improve accuracy for the "re-transcribe for accuracy" flow (FR-4); revisit there.

**English-only models force language "en".** With auto-detect, `.en` models report a meaningless language (we saw "fo"). The engine now uses "en" for any non-multilingual model.

**Threads default to min(cores, 8).** whisper.cpp gains little beyond 8. Tests that load models are serialized; in parallel they oversubscribe the CPU (100 s versus 4 s).

**Input format for M1: 16 kHz WAV only.** Other formats and sample rates come with ffmpeg in M2; the CLI prints the ffmpeg command to convert.

**Test audio is not committed.** Integration tests read `WHISPER_TEST_MODEL`, `WHISPER_TEST_AUDIO` and `WHISPER_TEST_VAD`, and skip when unset. M1 used LibriSpeech and public sample clips from Hugging Face (`Narsil/asr_dummy`: `1.flac`, `mlk.flac`). An accent test set with clear licensing is still to do (PRD section 14).

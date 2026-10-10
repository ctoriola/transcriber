use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::vad::{merge_spans, pack, Vad, MAX_CHUNK_SAMPLES, MERGE_GAP_SAMPLES, PACK_GAP_SAMPLES};
use crate::{Error, Result, SAMPLE_RATE};

#[derive(Debug, Clone)]
pub struct EngineOptions {
    /// Use the GPU backend compiled in (Metal/Vulkan) if any. Ignored on CPU-only builds.
    pub use_gpu: bool,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self { use_gpu: true }
    }
}

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    /// ISO 639-1 code such as "en", or `None` to auto-detect.
    pub language: Option<String>,
    /// Optional context for Whisper, such as names or jargon. Do not chain previous
    /// output here: it caused dropped sentences in M1 testing (see DECISIONS.md).
    pub initial_prompt: Option<String>,
    pub n_threads: usize,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        Self {
            language: None,
            initial_prompt: None,
            n_threads: default_threads(),
        }
    }
}

/// Physical-ish core count, capped: whisper.cpp gains little beyond 8 threads.
pub fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// Whisper's probability that this segment contains no speech.
    pub no_speech_prob: f32,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub segments: Vec<Segment>,
    /// Language Whisper used (the detected one when auto-detecting).
    pub language: Option<String>,
}

impl Transcript {
    pub fn text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A loaded Whisper model. Load once and reuse across transcriptions.
pub struct Engine {
    ctx: WhisperContext,
}

impl Engine {
    pub fn load(model_path: impl AsRef<Path>, options: &EngineOptions) -> Result<Self> {
        crate::silence_native_logs();

        let path = model_path.as_ref();
        if !path.is_file() {
            return Err(Error::ModelNotFound(path.display().to_string()));
        }
        let mut params = WhisperContextParameters::default();
        params.use_gpu(options.use_gpu);
        let ctx = WhisperContext::new_with_params(path, params)?;
        Ok(Self { ctx })
    }

    pub fn is_multilingual(&self) -> bool {
        self.ctx.is_multilingual()
    }

    /// Transcribe 16 kHz mono samples. Timestamps are relative to the start of `samples`.
    pub fn transcribe(&self, samples: &[f32], options: &TranscribeOptions) -> Result<Transcript> {
        let language = match options.language.as_deref() {
            None | Some("auto") => "auto",
            Some(code) => {
                if whisper_rs::get_lang_id(code).is_none() {
                    return Err(Error::Audio(format!("unknown language code: {code}")));
                }
                code
            }
        };
        // English-only (.en) models cannot detect language; auto would report garbage.
        let language = if self.is_multilingual() {
            language
        } else {
            "en"
        };

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(options.n_threads as i32);
        params.set_language(Some(language));
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Hallucination guards (PRD section 14).
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        if let Some(prompt) = options.initial_prompt.as_deref() {
            params.set_initial_prompt(prompt);
        }

        let mut state = self.ctx.create_state()?;
        state.full(params, samples)?;

        let mut segments = Vec::new();
        for seg in state.as_iter() {
            segments.push(Segment {
                // Whisper reports centiseconds.
                start_ms: (seg.start_timestamp().max(0) as u64) * 10,
                end_ms: (seg.end_timestamp().max(0) as u64) * 10,
                text: seg.to_str_lossy()?.trim().to_string(),
                no_speech_prob: seg.no_speech_probability(),
            });
        }
        let language =
            whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string);
        Ok(Transcript { segments, language })
    }
}

impl Engine {
    /// Transcribe only the speech in `samples`: VAD finds speech spans, which are
    /// packed into buffers of at most 25 s and transcribed in order. Timestamps are
    /// relative to the start
    /// of `samples`. Skipping non-speech is what keeps Whisper from hallucinating
    /// text on silence.
    pub fn transcribe_speech(
        &self,
        vad: &mut Vad,
        samples: &[f32],
        options: &TranscribeOptions,
    ) -> Result<Transcript> {
        let spans = vad.speech_spans(samples)?;
        let pieces = merge_spans(&spans, MAX_CHUNK_SAMPLES, MERGE_GAP_SAMPLES);
        let mut out = Transcript {
            segments: Vec::new(),
            language: None,
        };
        let ms_to_samples = |ms: u64| (ms * SAMPLE_RATE as u64 / 1000) as usize;
        let samples_to_ms = |n: usize| n as u64 * 1000 / SAMPLE_RATE as u64;
        for chunk in pack(&pieces, MAX_CHUNK_SAMPLES, PACK_GAP_SAMPLES) {
            let part = self.transcribe(&chunk.build(samples), options)?;
            if !part.text().is_empty() {
                out.language = out.language.or(part.language);
            }
            out.segments
                .extend(part.segments.into_iter().map(|mut seg| {
                    seg.start_ms = samples_to_ms(chunk.to_original(ms_to_samples(seg.start_ms)));
                    seg.end_ms = samples_to_ms(chunk.to_original(ms_to_samples(seg.end_ms)));
                    seg
                }));
        }
        Ok(out)
    }
}

/// Backends compiled into whisper.cpp, for diagnostics.
pub fn system_info() -> &'static str {
    whisper_rs::print_system_info()
}

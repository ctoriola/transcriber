//! Transcription engine: audio loading, Whisper inference and timestamp formatting.
//!
//! M1 scope: transcribe a 16 kHz WAV file with a GGML Whisper model and measure speed.

pub mod audio;
pub mod timestamp;
pub mod vad;
pub mod whisper;

pub use audio::{load_wav, Audio, SAMPLE_RATE};
pub use vad::Vad;
pub use whisper::{Engine, EngineOptions, Segment, TranscribeOptions, Transcript};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("audio: {0}")]
    Audio(String),
    #[error("wav: {0}")]
    Wav(#[from] hound::Error),
    #[error("whisper: {0}")]
    Whisper(#[from] whisper_rs::WhisperError),
    #[error("model file not found: {0}")]
    ModelNotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Route whisper.cpp/ggml logging away from stderr. Called before loading any model.
fn silence_native_logs() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(whisper_rs::install_logging_hooks);
}

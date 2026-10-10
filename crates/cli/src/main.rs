//! M1 engine spike CLI.
//!
//!   transcriber-cli transcribe <model.bin> <audio.wav> [options]
//!   transcriber-cli bench <audio.wav> <model.bin>... [options]
//!   transcriber-cli info

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use engine::timestamp::format_ms;
use engine::{load_wav, Engine, EngineOptions, TranscribeOptions, Transcript, Vad};

const USAGE: &str = "\
usage:
  transcriber-cli transcribe <model.bin> <audio.wav> [options]
  transcriber-cli bench <audio.wav> <model.bin>... [options]
  transcriber-cli info

options:
  --lang CODE    language code (e.g. en); default: auto-detect
  --threads N    CPU threads; default: min(cores, 8)
  --cpu          disable GPU acceleration
  --vad PATH     Silero VAD model; skips non-speech (prevents hallucination on silence)

Audio must be a 16 kHz WAV. Convert with:
  ffmpeg -i input -ar 16000 -ac 1 -c:a pcm_s16le out.wav";

struct Args {
    positional: Vec<String>,
    engine: EngineOptions,
    transcribe: TranscribeOptions,
    vad: Option<PathBuf>,
}

impl Args {
    /// Transcribe with VAD when `--vad` was given, otherwise run Whisper on all audio.
    fn run(
        &self,
        engine: &Engine,
        vad: Option<&mut Vad>,
        samples: &[f32],
    ) -> engine::Result<Transcript> {
        match vad {
            Some(vad) => engine.transcribe_speech(vad, samples, &self.transcribe),
            None => engine.transcribe(samples, &self.transcribe),
        }
    }

    fn load_vad(&self) -> engine::Result<Option<Vad>> {
        self.vad
            .as_ref()
            .map(|p| Vad::load(p, self.transcribe.n_threads))
            .transpose()
    }
}

fn parse(args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut out = Args {
        positional: Vec::new(),
        engine: EngineOptions::default(),
        transcribe: TranscribeOptions::default(),
        vad: None,
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lang" => {
                let v = args.next().ok_or("--lang needs a value")?;
                out.transcribe.language = (v != "auto").then_some(v);
            }
            "--threads" => {
                let v = args.next().ok_or("--threads needs a value")?;
                out.transcribe.n_threads = v.parse().map_err(|_| format!("bad --threads: {v}"))?;
            }
            "--cpu" => out.engine.use_gpu = false,
            "--vad" => {
                let v = args.next().ok_or("--vad needs a value")?;
                out.vad = Some(v.into());
            }
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with("--") => return Err(format!("unknown option: {s}")),
            _ => out.positional.push(arg),
        }
    }
    Ok(out)
}

fn main() -> ExitCode {
    let mut argv = std::env::args().skip(1);
    let command = argv.next().unwrap_or_default();
    let args = match parse(argv) {
        Ok(a) => a,
        Err(e) => return usage_error(&e),
    };
    let result = match (command.as_str(), args.positional.as_slice()) {
        ("transcribe", [model, audio]) => transcribe(model.as_ref(), audio.as_ref(), &args),
        ("bench", [audio, models @ ..]) if !models.is_empty() => {
            let models: Vec<PathBuf> = models.iter().map(PathBuf::from).collect();
            bench(audio.as_ref(), &models, &args)
        }
        ("info", []) => {
            println!(
                "whisper.cpp system info: {}",
                engine::whisper::system_info()
            );
            Ok(())
        }
        _ => return usage_error(""),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error(msg: &str) -> ExitCode {
    if !msg.is_empty() {
        eprintln!("error: {msg}\n");
    }
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn transcribe(model: &Path, audio: &Path, args: &Args) -> engine::Result<()> {
    let audio = load_wav(audio)?;
    let engine = Engine::load(model, &args.engine)?;
    let mut vad = args.load_vad()?;
    let started = Instant::now();
    let transcript = args.run(&engine, vad.as_mut(), &audio.samples)?;
    let elapsed = started.elapsed().as_secs_f64();

    for seg in &transcript.segments {
        println!(
            "[{} --> {}]  {}",
            format_ms(seg.start_ms),
            format_ms(seg.end_ms),
            seg.text
        );
    }
    eprintln!(
        "\nlanguage: {}  audio: {:.1}s  transcribed in {:.1}s ({:.1}x real time)",
        transcript.language.as_deref().unwrap_or("?"),
        audio.duration_secs(),
        elapsed,
        audio.duration_secs() / elapsed
    );
    Ok(())
}

fn bench(audio_path: &Path, models: &[PathBuf], args: &Args) -> engine::Result<()> {
    let audio = load_wav(audio_path)?;
    let duration = audio.duration_secs();
    println!(
        "whisper.cpp system info: {}",
        engine::whisper::system_info()
    );
    println!(
        "audio: {} ({duration:.1}s)  threads: {}  gpu: {}  vad: {}\n",
        audio_path.display(),
        args.transcribe.n_threads,
        args.engine.use_gpu,
        args.vad.is_some()
    );
    let mut vad = args.load_vad()?;
    println!(
        "{:<28} {:>8} {:>12} {:>14}",
        "model", "load s", "transcribe s", "x real time"
    );

    for model in models {
        let name = model.file_name().unwrap_or_default().to_string_lossy();
        let started = Instant::now();
        let engine = Engine::load(model, &args.engine)?;
        let load = started.elapsed().as_secs_f64();

        let started = Instant::now();
        args.run(&engine, vad.as_mut(), &audio.samples)?;
        let run = started.elapsed().as_secs_f64();

        println!(
            "{name:<28} {load:>8.2} {run:>12.2} {:>14.2}",
            duration / run
        );
    }
    println!("\n\"x real time\" above 1.0 means faster than real time.");
    Ok(())
}

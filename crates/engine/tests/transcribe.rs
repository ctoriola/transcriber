//! Integration tests that need a real model. They are skipped unless
//! `WHISPER_TEST_MODEL` points to a GGML model file (tiny.en is enough).
//! `WHISPER_TEST_AUDIO` optionally points to a 16 kHz speech WAV, and
//! `WHISPER_TEST_VAD` to the Silero VAD model (ggml-silero-*.bin).

use std::sync::{Mutex, MutexGuard};

use engine::{load_wav, Engine, EngineOptions, TranscribeOptions, Vad, SAMPLE_RATE};

/// Model tests run one at a time: each uses all cores, and running them in
/// parallel oversubscribes the CPU badly (about 30x slower).
static SERIAL: Mutex<()> = Mutex::new(());

fn model() -> Option<(MutexGuard<'static, ()>, Engine)> {
    let path = std::env::var("WHISPER_TEST_MODEL").ok()?;
    let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    Some((
        guard,
        Engine::load(path, &EngineOptions::default()).expect("load model"),
    ))
}

fn english() -> TranscribeOptions {
    TranscribeOptions {
        language: Some("en".into()),
        ..Default::default()
    }
}

#[test]
fn missing_model_is_a_clear_error() {
    let err = Engine::load("/nonexistent/model.bin", &EngineOptions::default())
        .err()
        .expect("should fail");
    assert!(err.to_string().contains("not found"), "{err}");
}

#[test]
fn rejects_unknown_language() {
    let Some((_serial, engine)) = model() else {
        return;
    };
    let opts = TranscribeOptions {
        language: Some("xx".into()),
        ..Default::default()
    };
    assert!(engine.transcribe(&[0.0; 16_000], &opts).is_err());
}

#[test]
fn speech_has_ordered_timestamps_within_audio() {
    let Some((_serial, engine)) = model() else {
        return;
    };
    let Ok(audio_path) = std::env::var("WHISPER_TEST_AUDIO") else {
        return;
    };
    let audio = load_wav(audio_path).unwrap();
    let transcript = engine.transcribe(&audio.samples, &english()).unwrap();

    assert!(!transcript.text().is_empty());
    let duration_ms = (audio.duration_secs() * 1000.0) as u64;
    let mut prev_end = 0;
    for seg in &transcript.segments {
        assert!(seg.start_ms <= seg.end_ms, "{seg:?}");
        assert!(seg.start_ms >= prev_end, "segments overlap: {seg:?}");
        // Whisper rounds to 10 ms and may run slightly past the end.
        assert!(
            seg.end_ms <= duration_ms + 1000,
            "{seg:?} beyond {duration_ms} ms"
        );
        prev_end = seg.end_ms;
    }
}

fn vad() -> Option<Vad> {
    let path = std::env::var("WHISPER_TEST_VAD").ok()?;
    Some(Vad::load(path, 4).expect("load VAD model"))
}

#[test]
fn whisper_alone_may_hallucinate_on_silence() {
    // Documents why VAD is required: no assertion, just prints what Whisper produced.
    let Some((_serial, engine)) = model() else {
        return;
    };
    let silence = vec![0.0f32; SAMPLE_RATE as usize * 10];
    let transcript = engine.transcribe(&silence, &english()).unwrap();
    eprintln!(
        "whisper on 10 s of silence without VAD: {:?}",
        transcript.text()
    );
}

#[test]
fn silence_with_vad_produces_no_text() {
    // Without VAD, Whisper often hallucinates on silence (tiny: "you"); VAD is the guard.
    let Some((_serial, engine)) = model() else {
        return;
    };
    let Some(mut vad) = vad() else { return };
    let silence = vec![0.0f32; SAMPLE_RATE as usize * 10];
    let transcript = engine
        .transcribe_speech(&mut vad, &silence, &english())
        .unwrap();
    assert_eq!(
        transcript.text(),
        "",
        "hallucinated on silence: {:?}",
        transcript.segments
    );
}

#[test]
fn vad_keeps_speech_timestamps_relative_to_original_audio() {
    let Some((_serial, engine)) = model() else {
        return;
    };
    let Some(mut vad) = vad() else { return };
    let Ok(audio_path) = std::env::var("WHISPER_TEST_AUDIO") else {
        return;
    };
    let speech = load_wav(audio_path).unwrap().samples;

    // 5 s of silence, then the speech.
    let lead = SAMPLE_RATE as usize * 5;
    let mut padded = vec![0.0f32; lead];
    padded.extend_from_slice(&speech);

    let transcript = engine
        .transcribe_speech(&mut vad, &padded, &english())
        .unwrap();
    assert!(!transcript.text().is_empty());
    let first = &transcript.segments[0];
    assert!(
        first.start_ms >= 4_000,
        "first segment should start near 5 s: {first:?}"
    );
}

#[test]
fn english_only_model_reports_english_when_auto_detecting() {
    let Some((_serial, engine)) = model() else {
        return;
    };
    if engine.is_multilingual() {
        return;
    }
    let transcript = engine
        .transcribe(&[0.0; 16_000], &TranscribeOptions::default())
        .unwrap();
    assert_eq!(transcript.language.as_deref(), Some("en"));
}

#[test]
fn vad_packing_keeps_every_repeated_sentence() {
    // Every sentence must survive VAD packing. (The 200 ms-gap and prompt-chaining
    // losses recorded in DECISIONS.md showed up with two different clips, not
    // one repeated clip, so this is a completeness check, not a reproduction.)
    let Some((_serial, engine)) = model() else {
        return;
    };
    let Some(mut vad) = vad() else { return };
    let Ok(audio_path) = std::env::var("WHISPER_TEST_AUDIO") else {
        return;
    };
    let speech = load_wav(audio_path).unwrap().samples;
    let reference = engine.transcribe(&speech, &english()).unwrap().text();
    // A plain word from the start of the sentence: rare words are spelled
    // differently from run to run.
    let word = reference
        .split_whitespace()
        .map(str::to_lowercase)
        .find(|w| w.len() >= 4 && w.chars().all(|c| c.is_ascii_alphabetic()))
        .unwrap();

    let mut audio = Vec::new();
    for gap_secs in [0, 2, 3, 5, 0] {
        audio.extend(std::iter::repeat_n(0.0f32, SAMPLE_RATE as usize * gap_secs));
        audio.extend_from_slice(&speech);
    }
    let text = engine
        .transcribe_speech(&mut vad, &audio, &english())
        .unwrap()
        .text()
        .to_lowercase();
    assert_eq!(
        text.matches(&word).count(),
        5,
        "{word:?} not found 5 times in: {text}"
    );
}

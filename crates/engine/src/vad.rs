//! Voice activity detection with whisper.cpp's built-in Silero VAD.
//!
//! whisper.cpp's own `params.vad` only applies in `whisper_full`, not in the
//! `whisper_full_with_state` call that whisper-rs uses, so we run the standalone
//! VAD context and feed Whisper the speech spans ourselves.
//!
//! Whisper always encodes a 30 s window, so sending each short utterance on its
//! own wastes most of the work. Speech pieces are packed into buffers of up to
//! 25 s, separated by a short silence, and timestamps are mapped back.

use std::ops::Range;
use std::path::Path;

use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use crate::{Error, Result, SAMPLE_RATE};

/// Maximum audio sent to Whisper in one call (PRD FR-2: about 25 seconds).
pub const MAX_CHUNK_SAMPLES: usize = SAMPLE_RATE as usize * 25;
/// Speech spans separated by at most this are kept as one piece, real pause included.
pub const MERGE_GAP_SAMPLES: usize = SAMPLE_RATE as usize;
/// Silence inserted between packed pieces. With 200 ms, base and small dropped
/// whole sentences after a boundary; 500 ms and up kept everything (see DECISIONS.md).
pub const PACK_GAP_SAMPLES: usize = SAMPLE_RATE as usize;

pub struct Vad {
    ctx: WhisperVadContext,
}

impl Vad {
    pub fn load(model_path: impl AsRef<Path>, n_threads: usize) -> Result<Self> {
        crate::silence_native_logs();
        let path = model_path.as_ref();
        if !path.is_file() {
            return Err(Error::ModelNotFound(path.display().to_string()));
        }
        let mut params = WhisperVadContextParams::new();
        params.set_n_threads(n_threads as i32);
        let ctx = WhisperVadContext::new(&path.to_string_lossy(), params)?;
        Ok(Self { ctx })
    }

    /// Sample ranges that contain speech, in order.
    pub fn speech_spans(&mut self, samples: &[f32]) -> Result<Vec<Range<usize>>> {
        let segments = self
            .ctx
            .segments_from_samples(WhisperVadParams::default(), samples)?;
        let cs_to_samples =
            |cs: f32| ((cs.max(0.0) * SAMPLE_RATE as f32 / 100.0) as usize).min(samples.len());
        Ok(segments
            .map(|s| cs_to_samples(s.start)..cs_to_samples(s.end))
            .filter(|r| !r.is_empty())
            .collect())
    }
}

/// Merge speech spans separated by at most `merge_gap` samples while the result
/// stays within `max_len`, and split any longer span into `max_len` pieces.
pub fn merge_spans(spans: &[Range<usize>], max_len: usize, merge_gap: usize) -> Vec<Range<usize>> {
    assert!(max_len > 0);
    let mut chunks: Vec<Range<usize>> = Vec::new();
    for span in spans {
        let mut start = span.start;
        while start < span.end {
            let end = span.end.min(start + max_len);
            match chunks.last_mut() {
                Some(last) if start - last.end <= merge_gap && end - last.start <= max_len => {
                    last.end = end;
                }
                _ => chunks.push(start..end),
            }
            start = end;
        }
    }
    chunks
}

/// Several pieces of the original audio, packed into one buffer for Whisper.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// (offset in the packed buffer, range in the original audio), in order.
    pub pieces: Vec<(usize, Range<usize>)>,
}

impl Chunk {
    /// Packed buffer length in samples.
    pub fn len(&self) -> usize {
        self.pieces.last().map_or(0, |(at, r)| at + r.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Build the packed buffer: pieces in order, with silence in between.
    pub fn build(&self, samples: &[f32]) -> Vec<f32> {
        let mut buf = vec![0.0; self.len()];
        for (at, r) in &self.pieces {
            buf[*at..at + r.len()].copy_from_slice(&samples[r.clone()]);
        }
        buf
    }

    /// Map a sample position in the packed buffer to the original audio.
    /// Positions in an inserted gap map to the end of the preceding piece.
    pub fn to_original(&self, pos: usize) -> usize {
        let i = self.pieces.partition_point(|(at, _)| *at <= pos).max(1) - 1;
        let (at, r) = &self.pieces[i];
        r.start + pos.saturating_sub(*at).min(r.len())
    }
}

/// Pack pieces, in order, into chunks of at most `max_len` samples with `gap`
/// samples of silence between pieces. Each piece must be at most `max_len`.
pub fn pack(pieces: &[Range<usize>], max_len: usize, gap: usize) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = Vec::new();
    for piece in pieces {
        assert!(piece.len() <= max_len);
        match chunks.last_mut() {
            Some(chunk) if chunk.len() + gap + piece.len() <= max_len => {
                let at = chunk.len() + gap;
                chunk.pieces.push((at, piece.clone()));
            }
            _ => chunks.push(Chunk {
                pieces: vec![(0, piece.clone())],
            }),
        }
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert!(merge_spans(&[], 100, 10).is_empty());
    }

    #[test]
    fn merges_close_spans() {
        assert_eq!(
            merge_spans(&[0..10, 15..20, 50..60], 100, 10),
            vec![0..20, 50..60]
        );
    }

    #[test]
    fn does_not_merge_past_max_len() {
        assert_eq!(merge_spans(&[0..60, 65..100], 80, 10), vec![0..60, 65..100]);
    }

    #[test]
    fn splits_long_span() {
        assert_eq!(
            merge_spans(&[Range { start: 0, end: 250 }], 100, 10),
            vec![0..100, 100..200, 200..250]
        );
    }

    #[test]
    fn covers_every_speech_sample() {
        let spans = [3..40, 41..170, 200..201, 205..999];
        let chunks = merge_spans(&spans, 64, 8);
        for span in &spans {
            for i in span.clone() {
                assert!(chunks.iter().any(|c| c.contains(&i)), "sample {i} dropped");
            }
        }
        assert!(chunks.iter().all(|c| c.len() <= 64));
        assert!(chunks.windows(2).all(|w| w[0].end <= w[1].start));
    }

    #[test]
    fn packs_pieces_with_gaps() {
        let chunks = pack(&[0..10, 50..60, 100..170], 90, 5);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].pieces, vec![(0, 0..10), (15, 50..60)]);
        assert_eq!(chunks[0].len(), 25);
        assert_eq!(chunks[1].pieces, vec![(0, 100..170)]);
    }

    #[test]
    fn build_places_samples_and_silence() {
        let samples: Vec<f32> = (0..20).map(|i| i as f32).collect();
        let chunk = Chunk {
            pieces: vec![(0, 2..4), (4, 10..12)],
        };
        assert_eq!(chunk.build(&samples), vec![2.0, 3.0, 0.0, 0.0, 10.0, 11.0]);
    }

    #[test]
    fn maps_positions_back_to_original() {
        let chunk = Chunk {
            pieces: vec![(0, 100..110), (15, 500..520)],
        };
        assert_eq!(chunk.to_original(0), 100);
        assert_eq!(chunk.to_original(9), 109);
        assert_eq!(chunk.to_original(12), 110); // in the gap: end of previous piece
        assert_eq!(chunk.to_original(15), 500);
        assert_eq!(chunk.to_original(34), 519);
        assert_eq!(chunk.to_original(99), 520); // past the end: clamped
    }

    #[test]
    fn packing_covers_every_speech_sample_once() {
        let spans = [3..40, 41..170, 200..201, 205..999, 3000..3010];
        let pieces = merge_spans(&spans, 64, 8);
        let chunks = pack(&pieces, 200, 7);
        let mut seen = vec![0u8; 4000];
        for chunk in &chunks {
            assert!(chunk.len() <= 200);
            for (at, r) in &chunk.pieces {
                for (k, orig) in r.clone().enumerate() {
                    assert_eq!(chunk.to_original(at + k), orig);
                    seen[orig] += 1;
                }
            }
        }
        for span in &spans {
            for i in span.clone() {
                assert_eq!(seen[i], 1, "sample {i}");
            }
        }
    }
}

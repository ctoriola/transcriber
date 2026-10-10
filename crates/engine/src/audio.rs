use std::path::Path;

use crate::{Error, Result};

/// Whisper's required input sample rate.
pub const SAMPLE_RATE: u32 = 16_000;

/// Mono f32 samples at [`SAMPLE_RATE`].
#[derive(Debug, Clone)]
pub struct Audio {
    pub samples: Vec<f32>,
}

impl Audio {
    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / SAMPLE_RATE as f64
    }
}

/// Load a 16 kHz WAV file (int 16/24/32-bit or float 32-bit, any channel count),
/// downmixing to mono. Other sample rates are rejected; arbitrary formats are
/// decoded through ffmpeg in M2.
pub fn load_wav(path: impl AsRef<Path>) -> Result<Audio> {
    let reader = hound::WavReader::open(path.as_ref())?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE {
        return Err(Error::Audio(format!(
            "sample rate is {} Hz, expected {SAMPLE_RATE} Hz \
             (convert with: ffmpeg -i in.wav -ar 16000 -ac 1 -c:a pcm_s16le out.wav)",
            spec.sample_rate
        )));
    }
    if spec.channels == 0 {
        return Err(Error::Audio("wav has zero channels".into()));
    }

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .into_samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()?
        }
    };

    Ok(Audio {
        samples: downmix(&interleaved, spec.channels as usize),
    })
}

/// Average interleaved channels into mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("engine-audio-{}-{name}", std::process::id()))
    }

    fn write_wav(path: &Path, spec: hound::WavSpec, samples: &[i16]) {
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn loads_mono_16k_i16() {
        let path = temp_path("mono.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        write_wav(&path, spec, &[0, i16::MAX, i16::MIN, 16384]);
        let audio = load_wav(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(audio.samples.len(), 4);
        assert_eq!(audio.samples[0], 0.0);
        assert!((audio.samples[1] - 1.0).abs() < 1e-4);
        assert_eq!(audio.samples[2], -1.0);
        assert_eq!(audio.samples[3], 0.5);
    }

    #[test]
    fn downmixes_stereo() {
        let path = temp_path("stereo.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        write_wav(&path, spec, &[16384, 0, -16384, -16384]);
        let audio = load_wav(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(audio.samples, vec![0.25, -0.5]);
    }

    #[test]
    fn rejects_wrong_sample_rate() {
        let path = temp_path("44k.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        write_wav(&path, spec, &[0; 10]);
        let err = load_wav(&path).unwrap_err();
        std::fs::remove_file(&path).ok();

        assert!(err.to_string().contains("44100"), "{err}");
    }

    #[test]
    fn duration() {
        let audio = Audio {
            samples: vec![0.0; 24_000],
        };
        assert_eq!(audio.duration_secs(), 1.5);
    }
}

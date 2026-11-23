use anyhow::Result;
use hound::{WavReader, WavSpec, WavWriter};
use log::debug;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::Path;

/// Read a WAV file and return normalised f32 samples.
pub fn read_wav_samples<P: AsRef<Path>>(file_path: P) -> Result<Vec<f32>> {
    let reader = WavReader::open(file_path.as_ref())?;
    let samples = reader
        .into_samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<Result<Vec<f32>, _>>()?;
    Ok(samples)
}

/// Verify a WAV file by reading it back and checking the sample count.
pub fn verify_wav_file<P: AsRef<Path>>(file_path: P, expected_samples: usize) -> Result<()> {
    let reader = WavReader::open(file_path.as_ref())?;
    let actual_samples = reader.len() as usize;
    if actual_samples != expected_samples {
        anyhow::bail!(
            "WAV sample count mismatch: expected {}, got {}",
            expected_samples,
            actual_samples
        );
    }
    Ok(())
}

/// Save audio samples as a WAV file
pub fn save_wav_file<P: AsRef<Path>>(file_path: P, samples: &[f32]) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = WavWriter::create(file_path.as_ref(), spec)?;

    // Convert f32 samples to i16 for WAV
    for sample in samples {
        let sample_i16 = (sample * i16::MAX as f32) as i16;
        writer.write_sample(sample_i16)?;
    }

    writer.finalize()?;
    debug!("Saved WAV file: {:?}", file_path.as_ref());
    Ok(())
}

/// Typed error boundary distinguishing a failed exclusive open (before this process
/// created or owned any file) from a write/finalize failure after `create_new(true)`
/// succeeded (where this process owns the partial file and must clean it up through
/// the root-bounded deletion path).
#[derive(Debug)]
pub enum ExclusiveWavError {
    OpenFailed(anyhow::Error),
    WriteFailedAfterCreate(anyhow::Error),
}

impl ExclusiveWavError {
    pub fn is_already_exists(&self) -> bool {
        matches!(
            self,
            Self::OpenFailed(err)
                if err
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io_e| io_e.kind() == std::io::ErrorKind::AlreadyExists)
        )
    }
}

impl std::fmt::Display for ExclusiveWavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenFailed(err) => write!(f, "exclusive WAV open failed: {err}"),
            Self::WriteFailedAfterCreate(err) => {
                write!(f, "exclusive WAV write failed after create: {err}")
            }
        }
    }
}

impl std::error::Error for ExclusiveWavError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OpenFailed(err) | Self::WriteFailedAfterCreate(err) => Some(err.as_ref()),
        }
    }
}

/// Exclusively create and write a new WAV file (`create_new(true)` + `O_NOFOLLOW` on Unix)
/// so concurrent takes can never collide with or follow a pre-existing file or symlink.
/// - Returns `Err(ExclusiveWavError::OpenFailed(_))` if `open()` fails, without ever
///   touching or unlinking the pre-existing path.
/// - Returns `Err(ExclusiveWavError::WriteFailedAfterCreate(_))` if writing or finalizing
///   fails after `create_new(true)` succeeded; closes the file handle and leaves
///   root-bounded cleanup to the caller so failed removals are never silently suppressed.
pub fn save_wav_file_exclusive<P: AsRef<Path>>(
    file_path: P,
    samples: &[f32],
) -> std::result::Result<(), ExclusiveWavError> {
    let path = file_path.as_ref();
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }

    let file = opts
        .open(path)
        .map_err(|e| ExclusiveWavError::OpenFailed(e.into()))?;

    let mut writer = WavWriter::new(BufWriter::new(file), spec)
        .map_err(|e| ExclusiveWavError::WriteFailedAfterCreate(e.into()))?;

    for sample in samples {
        let sample_i16 = (sample * i16::MAX as f32) as i16;
        writer
            .write_sample(sample_i16)
            .map_err(|e| ExclusiveWavError::WriteFailedAfterCreate(e.into()))?;
    }

    writer
        .finalize()
        .map_err(|e| ExclusiveWavError::WriteFailedAfterCreate(e.into()))?;
    debug!("Saved exclusive WAV file: {:?}", path);
    Ok(())
}

#[cfg(not(unix))]
pub fn unlink_regular_file_non_unix(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "failed to delete WAV file '{}': {}",
            path.display(),
            e
        )),
    }
}

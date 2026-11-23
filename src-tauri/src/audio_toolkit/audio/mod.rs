// Re-export all audio components
#[cfg(feature = "meetings")]
mod append_wav;
mod device;
mod recorder;
mod resampler;
mod utils;
mod visualizer;

#[cfg(feature = "meetings")]
pub use append_wav::AppendWav;

pub use device::{list_input_devices, list_output_devices, CpalDeviceInfo};
pub use recorder::{
    is_microphone_access_denied, is_no_input_device_error, AudioRecorder, SilentMicDetector,
    VadPolicy, SILENT_START_WINDOW_SAMPLES,
};
pub use resampler::FrameResampler;
#[cfg(not(unix))]
pub use utils::unlink_regular_file_non_unix;
pub use utils::{
    read_wav_samples, save_wav_file, save_wav_file_exclusive, verify_wav_file, ExclusiveWavError,
};
pub use visualizer::AudioVisualiser;

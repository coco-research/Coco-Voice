pub mod audio;
pub mod constants;
pub mod text;
pub mod utils;
pub mod vad;

#[cfg(not(unix))]
pub use audio::unlink_regular_file_non_unix;
pub use audio::{
    is_microphone_access_denied, is_no_input_device_error, list_input_devices, list_output_devices,
    read_wav_samples, save_wav_file, save_wav_file_exclusive, verify_wav_file, AudioRecorder,
    CpalDeviceInfo, ExclusiveWavError, VadPolicy,
};
pub use text::{
    apply_corrections, apply_custom_words, filter_transcription_output, resolve_take_language,
    rewrite_spoken_self_corrections,
};
#[allow(unused_imports)]
pub(crate) use text::{
    expand_saved_blocks, remember_spelling_correction, suggest_spelling_corrections,
};
pub use utils::get_cpal_host;
pub use vad::{SileroVad, VoiceActivityDetector};

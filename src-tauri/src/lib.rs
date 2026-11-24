mod actions;
pub mod agent_ask;
mod agent_ask_bridge;
pub mod app_profiles;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod apple_intelligence;
mod audio_feedback;
pub mod audio_toolkit;
#[cfg(feature = "meetings")]
mod calendar_commands;
#[cfg(all(feature = "meetings", target_os = "macos"))]
mod calendar_eventkit;
#[cfg(feature = "meetings")]
mod calendar_upcoming;
mod catalog;
mod cleanup_eval;
mod cleanup_resolver;
pub mod cli;
mod clipboard;
mod commands;
pub mod dictation_features;
pub mod first_run_and_updates;
mod fix_offer;
pub mod focused_text;
mod helpers;
mod input;
pub mod last_take;
mod llm_client;
#[cfg(target_os = "macos")]
mod local_llm;
mod managers;
pub mod mcp;
pub mod mcp_store;
#[cfg(feature = "meetings")]
pub mod meeting_append;
#[cfg(feature = "meetings")]
pub mod meeting_ask;
#[cfg(feature = "meetings")]
pub mod meeting_ask_index;
#[cfg(feature = "meetings")]
mod meeting_controller;
#[cfg(feature = "meetings")]
pub mod meeting_extras_commands;
#[cfg(feature = "meetings")]
mod meeting_feed;
#[cfg(feature = "meetings")]
pub mod meeting_history_ops;
#[cfg(feature = "meetings")]
pub mod meeting_import;
#[cfg(feature = "meetings")]
pub mod meeting_language;
#[cfg(feature = "meetings")]
mod meeting_notes;
#[cfg(feature = "meetings")]
mod meeting_notes_merge;
#[cfg(feature = "meetings")]
mod meeting_notes_pieces;
#[cfg(feature = "meetings")]
pub mod meeting_notes_runner;
#[cfg(feature = "meetings")]
pub mod meeting_notes_v42;
#[cfg(feature = "meetings")]
mod meeting_offer_panel;
#[cfg(feature = "meetings")]
pub mod meeting_pipeline;
#[cfg(feature = "meetings")]
pub mod meeting_retention;
#[cfg(feature = "meetings")]
mod meeting_session;
#[cfg(feature = "speaker-split")]
pub mod meeting_speakers;
#[cfg(feature = "speaker-split")]
pub mod meeting_speakers_engine;
#[cfg(feature = "meetings")]
pub mod meeting_todos;
#[cfg(feature = "meetings")]
mod meetings;
#[cfg(feature = "meetings")]
pub mod meetings_list;
#[cfg(all(feature = "meetings", target_os = "macos"))]
pub mod meetings_tap;
mod overlay;
pub mod portable;
mod repo_words;
mod settings;
mod shortcut;
mod signal_handle;
mod transcription_coordinator;
mod tray;
mod tray_i18n;
mod utils;
pub mod v020_commands;
#[cfg(test)]
mod y7_privacy_guards;

pub use cli::CliArgs;
#[cfg(debug_assertions)]
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri_specta::{collect_commands, collect_events, Builder};

use env_filter::Builder as EnvFilterBuilder;
use managers::audio::AudioRecordingManager;
use managers::history::HistoryManager;
use managers::model::ModelManager;
use managers::transcription::TranscriptionManager;
#[cfg(unix)]
use signal_hook::consts::{SIGUSR1, SIGUSR2};
#[cfg(unix)]
use signal_hook::iterator::Signals;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use tauri::image::Image;
pub use transcription_coordinator::TranscriptionCoordinator;

use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_log::{Builder as LogBuilder, RotationStrategy, Target, TargetKind};

use crate::settings::get_settings;

// Global atomic to store the file log level filter
// We use u8 to store the log::LevelFilter as a number
pub static FILE_LOG_LEVEL: AtomicU8 = AtomicU8::new(log::LevelFilter::Info as u8);

/// When `true`, log records are also forwarded to the webview via the
/// `log://log` event for the debug panel's live log viewer. Gated on debug
/// mode — the live log viewer is its only consumer and only exists in debug
/// mode — so normal runs never broadcast log records (which can include file
/// paths) onto the frontend event bus. Synced at startup
/// and whenever debug mode is toggled (see `shortcut::change_debug_mode_setting`).
pub static WEBVIEW_LOG_STREAMING: AtomicBool = AtomicBool::new(false);

/// Wipe log files that may hold dictated text, once. A failure only warns:
/// startup continues and the next launch tries again.
fn clear_legacy_transcript_logs(app: &AppHandle) {
    if settings::get_settings(app).old_logs_cleared {
        return;
    }
    let log_dir = match portable::app_log_dir(app) {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("Could not resolve the log directory; leaving old logs in place: {e}");
            return;
        }
    };
    if let Err(e) = settings::clear_old_voice_logs(&log_dir) {
        log::warn!("Could not clear old voice logs; will retry next launch: {e}");
        return;
    }
    settings::update_settings(app, |s| s.old_logs_cleared = true);
}

fn level_filter_from_u8(value: u8) -> log::LevelFilter {
    match value {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Error,
        2 => log::LevelFilter::Warn,
        3 => log::LevelFilter::Info,
        4 => log::LevelFilter::Debug,
        5 => log::LevelFilter::Trace,
        _ => log::LevelFilter::Trace,
    }
}

fn build_console_filter() -> env_filter::Filter {
    let mut builder = EnvFilterBuilder::new();

    match std::env::var("RUST_LOG") {
        Ok(spec) if !spec.trim().is_empty() => {
            if let Err(err) = builder.try_parse(&spec) {
                log::warn!(
                    "Ignoring invalid RUST_LOG value '{}': {}. Falling back to info-level console logging",
                    spec,
                    err
                );
                builder.filter_level(log::LevelFilter::Info);
            }
        }
        _ => {
            builder.filter_level(log::LevelFilter::Info);
        }
    }

    builder.build()
}

fn show_main_window(app: &AppHandle) {
    if let Some(main_window) = app.get_webview_window("main") {
        if let Err(e) = main_window.unminimize() {
            log::error!("Failed to unminimize webview window: {}", e);
        }
        if let Err(e) = main_window.show() {
            log::error!("Failed to show webview window: {}", e);
        }
        if let Err(e) = main_window.set_focus() {
            log::error!("Failed to focus webview window: {}", e);
        }
        #[cfg(target_os = "macos")]
        {
            if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
                log::error!("Failed to set activation policy to Regular: {}", e);
            }
        }
        return;
    }

    let webview_labels = app.webview_windows().keys().cloned().collect::<Vec<_>>();
    log::error!(
        "Main window not found. Webview labels: {:?}",
        webview_labels
    );
}

#[allow(unused_variables)]
fn should_force_show_permissions_window(app: &AppHandle) -> bool {
    #[cfg(target_os = "windows")]
    {
        let model_manager = app.state::<Arc<ModelManager>>();
        let has_downloaded_models = model_manager
            .get_available_models()
            .iter()
            .any(|model| model.is_downloaded);

        if !has_downloaded_models {
            return false;
        }

        let status = commands::audio::get_windows_microphone_permission_status();
        if status.supported && status.overall_access == commands::audio::PermissionAccess::Denied {
            log::info!(
                "Windows microphone permissions are denied; forcing main window visible for onboarding"
            );
            return true;
        }
    }

    false
}

fn initialize_core_logic(app_handle: &AppHandle) {
    // Note: Enigo (keyboard/mouse simulation) is NOT initialized here.
    // The frontend is responsible for calling the `initialize_enigo` command
    // after onboarding completes. This avoids triggering permission dialogs
    // on macOS before the user is ready.

    // Initialize the managers. The audio recorder receives the streaming router
    // explicitly, so always-on microphone startup can wire live-preview frames
    // even before Tauri state is populated.
    let model_manager =
        Arc::new(ModelManager::new(app_handle).expect("Failed to initialize model manager"));
    let transcription_manager = Arc::new(
        TranscriptionManager::new(app_handle, model_manager.clone())
            .expect("Failed to initialize transcription manager"),
    );
    let recording_manager = Arc::new(
        AudioRecordingManager::new(app_handle, transcription_manager.stream_router())
            .expect("Failed to initialize recording manager"),
    );
    let history_manager =
        Arc::new(HistoryManager::new(app_handle).expect("Failed to initialize history manager"));

    // Initialize the transcribe-cpp native backend (logging + backend module
    // registration) once, before any whisper model is loaded.
    managers::transcription::init_transcribe_backend();

    // Apply accelerator preferences before any model loads
    managers::transcription::apply_accelerator_settings(app_handle);

    // Add managers to Tauri's managed state
    app_handle.manage(recording_manager.clone());
    app_handle.manage(model_manager.clone());
    app_handle.manage(transcription_manager.clone());
    app_handle.manage(history_manager.clone());
    app_handle.manage(last_take::LastTake::new());
    app_handle.manage(Mutex::new(repo_words::RepoSymbolsCache::new()));
    app_handle.manage(Mutex::new(fix_offer::FixOfferQueue::new()));
    app_handle.manage(Arc::new(fix_offer::WatcherCancellation::new()));
    Arc::clone(&history_manager).start_sweeper();
    app_handle.manage(tray::CurrentTrayIconState::new());
    #[cfg(unix)]
    {
        let s = settings::get_settings(app_handle);
        let enabled = agent_ask_bridge::effective_agents_can_ask(
            s.agents_can_ask_enabled,
            s.agent_ask_enabled,
        );
        let key = s
            .bindings
            .get("transcribe")
            .map(|b| b.current_binding.clone())
            .unwrap_or_else(|| "fn".to_string());
        let state = Arc::new(Mutex::new(agent_ask_bridge::AgentAskBridgeState::new(
            enabled, key,
        )));
        if let Ok(data_dir) = portable::app_data_dir(app_handle) {
            let ah_ev = app_handle.clone();
            if let Ok(server) = agent_ask_bridge::AgentAskServer::start(
                &data_dir,
                Arc::clone(&state),
                Arc::new(move |ev| {
                    if let Some(ov) = ah_ev.get_webview_window("recording_overlay") {
                        let _ = ov.emit("agent-ask-question", ev);
                    }
                }),
            ) {
                app_handle.manage(state);
                app_handle.manage(server);
            }
        }
    }
    #[cfg(feature = "meetings")]
    {
        app_handle.manage(Arc::new(meeting_controller::MeetingController::new(
            Box::new(meeting_session::MeetingSession::new(app_handle.clone())),
        )));
        app_handle.manage(Arc::new(meeting_pipeline::MeetingPipelineState::new()));
        // Open runs unfinished-row recovery. A later meeting start opens the
        // same file again; the update is a no-op once no row is still recording.
        match managers::meetings::MeetingsStore::open_app(app_handle) {
            Ok(store) => {
                let _ = meeting_history_ops::purge_expired_trash(
                    &store,
                    chrono::Utc::now().timestamp(),
                );
                let mut s = settings::get_settings(app_handle);
                if let Ok(true) = store.seed_stock_templates_once(s.meeting_templates_seeded) {
                    s.meeting_templates_seeded = true;
                    settings::write_settings(app_handle, s);
                }
            }
            Err(err) => {
                log::warn!("meetings store recovery failed: {err}");
            }
        }
        meeting_pipeline::sweep_interrupted_on_startup(app_handle);
        let purge_app = app_handle.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(24 * 60 * 60));
            if let Ok(store) = managers::meetings::MeetingsStore::open_app(&purge_app) {
                let _ = meeting_history_ops::purge_expired_trash(
                    &store,
                    chrono::Utc::now().timestamp(),
                );
            }
        });
    }

    // Note: Shortcuts are NOT initialized here.
    // The frontend is responsible for calling the `initialize_shortcuts` command
    // after permissions are confirmed (on macOS) or after onboarding completes.
    // This matches the pattern used for Enigo initialization.

    #[cfg(unix)]
    let signals = Signals::new([SIGUSR1, SIGUSR2]).unwrap();
    // Set up signal handlers for toggling transcription
    #[cfg(unix)]
    signal_handle::setup_signal_handler(app_handle.clone(), signals);

    // Apply macOS Accessory policy if starting hidden and tray is available.
    // If the tray icon is disabled, keep the dock icon so the user can reopen.
    #[cfg(target_os = "macos")]
    {
        let settings = settings::get_settings(app_handle);
        if settings.start_hidden && settings.show_tray_icon {
            let _ = app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
    }
    // Get the current theme to set the appropriate initial icon
    let initial_theme = tray::get_current_theme(app_handle);

    // Choose the appropriate initial icon based on theme
    let initial_icon_path = tray::get_icon_path(initial_theme, tray::TrayIconState::Idle);

    let tray = TrayIconBuilder::new()
        .icon(
            Image::from_path(
                app_handle
                    .path()
                    .resolve(initial_icon_path, tauri::path::BaseDirectory::Resource)
                    .unwrap(),
            )
            .unwrap(),
        )
        .tooltip(tray::tray_tooltip())
        .show_menu_on_left_click(true)
        .icon_as_template(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_app" | "settings" => {
                show_main_window(app);
            }
            "check_updates" => {
                let settings = settings::get_settings(app);
                if settings.update_checks_enabled {
                    show_main_window(app);
                    let _ = app.emit("check-for-updates", ());
                }
            }
            "paste_last_transcript" => {
                tray::paste_last_transcript(app);
            }
            "copy_last_transcript" => {
                tray::copy_last_transcript(app);
            }
            "unload_model" => {
                let transcription_manager = app.state::<Arc<TranscriptionManager>>();
                if !transcription_manager.is_model_loaded() {
                    log::warn!("No model is currently loaded.");
                    return;
                }
                match transcription_manager.unload_model() {
                    Ok(()) => log::info!("Model unloaded via tray."),
                    Err(e) => log::error!("Failed to unload model via tray: {}", e),
                }
            }
            "cancel" => {
                use crate::utils::cancel_current_operation;

                // Use centralized cancellation that handles all operations
                cancel_current_operation(app);
            }
            "quit" => {
                app.exit(0);
            }
            #[cfg(feature = "meetings")]
            "start_meeting" | "start_notes" => {
                // Same path as the meeting shortcut: consent only, never a recording.
                show_main_window(app);
                let _ = crate::meeting_controller::open_meeting_consent(app.clone());
            }
            #[cfg(feature = "meetings")]
            "stop_meeting" => {
                let app_clone = app.clone();
                std::thread::spawn(move || {
                    let _ = crate::meeting_controller::stop_meeting(app_clone);
                });
            }
            id if id.starts_with("model_select:") => {
                let model_id = id.strip_prefix("model_select:").unwrap().to_string();
                let current_model = settings::get_settings(app).selected_model;
                if model_id == current_model {
                    return;
                }
                let app_clone = app.clone();
                std::thread::spawn(move || {
                    match commands::models::switch_active_model(&app_clone, &model_id) {
                        Ok(()) => {
                            log::info!("Model switched to {} via tray.", model_id);
                        }
                        Err(e) => {
                            log::error!("Failed to switch model via tray: {}", e);
                        }
                    }
                    tray::update_tray_menu(&app_clone, None);
                });
            }
            _ => {}
        })
        .build(app_handle)
        .unwrap();
    app_handle.manage(tray);

    // Initialize tray menu with idle state
    utils::update_tray_menu(app_handle, None);

    // Apply show_tray_icon setting
    let settings = settings::get_settings(app_handle);
    if !settings.show_tray_icon {
        tray::set_tray_visibility(app_handle, false);
    }

    // Refresh tray menu when model state changes
    let app_handle_for_listener = app_handle.clone();
    app_handle.listen("model-state-changed", move |_| {
        tray::update_tray_menu(&app_handle_for_listener, None);
    });

    // Get the autostart manager and configure based on user setting
    let autostart_manager = app_handle.autolaunch();
    let settings = settings::get_settings(app_handle);

    if settings.autostart_enabled {
        // Enable autostart if user has opted in
        let _ = autostart_manager.enable();
    } else {
        // Disable autostart if user has opted out
        let _ = autostart_manager.disable();
    }

    // Create the recording overlay window (hidden by default)
    utils::create_recording_overlay(app_handle);
    #[cfg(feature = "meetings")]
    meeting_offer_panel::create_meeting_offer_panel(app_handle);
}

#[tauri::command]
#[specta::specta]
fn trigger_update_check(app: AppHandle) -> Result<(), String> {
    let settings = settings::get_settings(&app);
    if !settings.update_checks_enabled {
        return Ok(());
    }
    app.emit("check-for-updates", ())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
fn show_main_window_command(app: AppHandle) -> Result<(), String> {
    show_main_window(&app);
    Ok(())
}

/// Headless one-shot transcription for the `--transcribe-file` / `--list-devices`
/// path. Drives the same `TranscriptionManager::transcribe` the app uses; no
/// mic, no VAD, no download. Returns a process exit code (0 ok, 1 runtime
/// failure, 2 bad input/usage).
fn run_headless_transcription(app: &AppHandle, args: &CliArgs) -> i32 {
    use std::time::Instant;

    // --list-devices: print registered compute devices (with indices) and exit.
    // Useful on multi-GPU machines to discover the index for --device-index.
    if args.list_devices {
        let devices = crate::managers::transcription::describe_compute_devices();
        if devices.is_empty() {
            println!("No transcribe-cpp compute devices registered.");
        } else {
            println!("transcribe-cpp compute devices:");
            for d in &devices {
                println!("  {}", d);
            }
        }
        if args.transcribe_file.is_none() {
            return 0;
        }
    }

    // --list-models: print the model registry (catalog + on-disk + custom) with
    // their ids — the same ids `--model` accepts — then exit. `--json` emits the
    // full ModelInfo array for scripting.
    if args.list_models {
        let model_manager = app.state::<Arc<ModelManager>>();
        let models = model_manager.get_available_models();
        if args.json {
            match serde_json::to_string_pretty(&models) {
                Ok(s) => println!("{}", s),
                Err(e) => {
                    eprintln!("error: failed to serialize models: {}", e);
                    return 1;
                }
            }
        } else if models.is_empty() {
            println!("No models available.");
        } else {
            println!("Available models (✓ = installed):");
            let width = models.iter().map(|m| m.id.len()).max().unwrap_or(0);
            for m in &models {
                let mark = if m.is_downloaded { "✓" } else { " " };
                let rec = if m.is_recommended {
                    "  [recommended]"
                } else {
                    ""
                };
                println!(
                    "  {}  {:<width$}  {}{}",
                    mark,
                    m.id,
                    m.name,
                    rec,
                    width = width
                );
            }
        }
        if args.transcribe_file.is_none() {
            return 0;
        }
    }

    let Some(wav) = args.transcribe_file.clone() else {
        return 0;
    };

    // read_wav_samples reads 16-bit int samples and does no validation; the app
    // only ever saves 16 kHz mono 16-bit PCM, so reject anything else rather than
    // transcribe garbage / mis-time / mis-decode.
    match hound::WavReader::open(&wav) {
        Ok(reader) => {
            let spec = reader.spec();
            if spec.sample_rate != 16_000
                || spec.channels != 1
                || spec.bits_per_sample != 16
                || spec.sample_format != hound::SampleFormat::Int
            {
                eprintln!(
                    "error: expected 16 kHz mono 16-bit PCM WAV, got {} Hz / {} ch / {}-bit {:?}",
                    spec.sample_rate, spec.channels, spec.bits_per_sample, spec.sample_format
                );
                return 2;
            }
        }
        Err(e) => {
            eprintln!("error: cannot open {}: {}", wav.display(), e);
            return 2;
        }
    }

    let samples = match crate::audio_toolkit::read_wav_samples(&wav) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: failed to read {}: {}", wav.display(), e);
            return 2;
        }
    };
    let audio_secs = samples.len() as f64 / 16_000.0;

    let tm = app.state::<Arc<TranscriptionManager>>();

    let model_id = args
        .model
        .clone()
        .unwrap_or_else(|| get_settings(app).selected_model);
    if model_id.is_empty() {
        eprintln!("error: no model selected (pass --model or pick one in the app)");
        return 2;
    }

    // --device-index hard-selects a compute device by its --list-devices registry
    // index (transcribe-cpp / whisper-family models only; not persisted). Omit it
    // to use the persisted accelerator setting.
    let device_index = args.device_index;
    let requested_device = match device_index {
        Some(idx) => format!("index {}", idx),
        None => "settings".to_string(),
    };

    // Cold load (timed).
    let load_start = Instant::now();
    if let Err(e) = tm.load_model_with_device(&model_id, device_index) {
        eprintln!("error: load_model('{}') failed: {}", model_id, e);
        return 1;
    }
    let load_ms = load_start.elapsed().as_millis() as u64;
    let bound_backend = tm.current_backend();

    let runs = args.repeat.unwrap_or(1).max(1);
    let mut times_ms: Vec<u64> = Vec::new();
    let mut text = String::new();
    for i in 0..runs {
        // If the model's unload-timeout is "Immediately", transcribe() unloads
        // the engine after each run; reload (untimed) so repeats keep working
        // and the inference timing below stays clean.
        if !tm.is_model_loaded() {
            if let Err(e) = tm.load_model_with_device(&model_id, device_index) {
                eprintln!("error: reload before run {} failed: {}", i + 1, e);
                return 1;
            }
        }
        let t = Instant::now();
        match tm.transcribe(samples.clone()) {
            Ok(out) => text = out,
            Err(e) => {
                eprintln!("error: transcribe failed: {}", e);
                return 1;
            }
        }
        times_ms.push(t.elapsed().as_millis() as u64);
    }
    let best_ms = times_ms.iter().copied().min().unwrap_or(0);
    let rtf = if best_ms > 0 {
        audio_secs / (best_ms as f64 / 1000.0)
    } else {
        0.0
    };

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "model": model_id,
                "requested_device": requested_device,
                "bound_backend": bound_backend,
                "audio_secs": audio_secs,
                "load_ms": load_ms,
                "transcribe_ms": times_ms,
                "best_ms": best_ms,
                "rtf": rtf,
                "text": text,
            })
        );
    } else {
        println!(
            "model={} device={} backend={} audio={:.2}s load={}ms best={}ms rtf={:.2}x",
            model_id,
            requested_device,
            bound_backend.as_deref().unwrap_or("?"),
            audio_secs,
            load_ms,
            best_ms,
            rtf,
        );
        println!("text: {}", text);
    }
    0
}

/// Meetings command names registered below. Empty unless the `meetings` feature is on.
#[cfg_attr(not(test), allow(dead_code))]
fn meetings_command_names() -> &'static [&'static str] {
    #[cfg(feature = "speaker-split")]
    {
        &[
            "change_meetings_enabled_setting",
            "change_meeting_language_setting",
            "set_meeting_language_override_cmd",
            "meeting_language_plan",
            "open_meeting_consent",
            "confirm_meeting_start",
            "confirm_meeting_start_mic_only",
            "cancel_meeting_consent",
            "stop_meeting",
            "pause_meeting_cmd",
            "resume_meeting_cmd",
            "request_append_meeting_consent",
            "confirm_append_meeting_audio",
            "current_meeting_offer",
            "dismiss_meeting_offer",
            "meeting_status",
            "meeting_segments",
            "list_meetings",
            "search_meetings",
            "export_meeting_markdown",
            "delete_meeting",
            "rename_meeting",
            "meeting_retention_state",
            "change_delete_audio_after_notes_setting",
            "request_delete_all_meetings",
            "delete_all_meetings",
            "change_notes_markdown_folder_setting",
            "meeting_notes_snapshot",
            "save_meeting_notepad",
            "retry_meeting_notes",
            "catch_up_meeting",
            "pause_meeting",
            "resume_meeting",
            "append_meeting_audio",
            "list_trashed_meetings",
            "restore_trashed_meeting",
            "delete_transcript_span",
            "change_transcript_delete_days_setting",
            "import_meeting_notes",
            "ask_meetings",
            "list_meeting_todos",
            "complete_meeting_todo",
            "agent_access_state",
            "set_agent_access_enabled",
            "set_agent_access_scope",
            "mcp_command_line",
            "get_meeting_custom_words",
            "set_meeting_custom_words",
            "list_calendar_choices",
            "change_selected_calendars_setting",
            "mark_calendar_as_work_setting",
            "list_upcoming_meetings",
            "keep_upcoming_for_notes",
            "close_upcoming_meetings",
            "trash_meeting",
            "delete_meeting_forever",
            "list_trashed_meetings_v2",
            "restore_trashed_meeting_v2",
            "purge_expired_trash_now",
            "delete_transcript_segments",
            "delete_transcript_span_secure",
            "search_meeting_transcript",
            "list_meeting_folders",
            "create_meeting_folder",
            "rename_meeting_folder",
            "delete_meeting_folder",
            "set_meeting_folder",
            "list_meeting_templates",
            "create_meeting_template",
            "update_meeting_template",
            "delete_meeting_template",
            "set_folder_last_template",
            "ask_meetings_v2",
            "start_ask_by_voice",
            "cancel_ask_by_voice",
            "finish_ask_by_voice",
            "ask_meetings_by_voice",
            "count_importable_notes",
            "import_meeting_notes_batch",
            "list_meeting_speakers",
            "rename_meeting_speaker_slot",
            "set_meeting_offer_visible",
        ]
    }
    #[cfg(all(feature = "meetings", not(feature = "speaker-split")))]
    {
        &[
            "change_meetings_enabled_setting",
            "change_meeting_language_setting",
            "set_meeting_language_override_cmd",
            "meeting_language_plan",
            "open_meeting_consent",
            "confirm_meeting_start",
            "confirm_meeting_start_mic_only",
            "cancel_meeting_consent",
            "stop_meeting",
            "pause_meeting_cmd",
            "resume_meeting_cmd",
            "request_append_meeting_consent",
            "confirm_append_meeting_audio",
            "current_meeting_offer",
            "dismiss_meeting_offer",
            "meeting_status",
            "meeting_segments",
            "list_meetings",
            "search_meetings",
            "export_meeting_markdown",
            "delete_meeting",
            "rename_meeting",
            "meeting_retention_state",
            "change_delete_audio_after_notes_setting",
            "request_delete_all_meetings",
            "delete_all_meetings",
            "change_notes_markdown_folder_setting",
            "meeting_notes_snapshot",
            "save_meeting_notepad",
            "retry_meeting_notes",
            "catch_up_meeting",
            "pause_meeting",
            "resume_meeting",
            "append_meeting_audio",
            "list_trashed_meetings",
            "restore_trashed_meeting",
            "delete_transcript_span",
            "change_transcript_delete_days_setting",
            "import_meeting_notes",
            "ask_meetings",
            "list_meeting_todos",
            "complete_meeting_todo",
            "agent_access_state",
            "set_agent_access_enabled",
            "set_agent_access_scope",
            "mcp_command_line",
            "get_meeting_custom_words",
            "set_meeting_custom_words",
            "list_calendar_choices",
            "change_selected_calendars_setting",
            "mark_calendar_as_work_setting",
            "list_upcoming_meetings",
            "keep_upcoming_for_notes",
            "close_upcoming_meetings",
            "trash_meeting",
            "delete_meeting_forever",
            "list_trashed_meetings_v2",
            "restore_trashed_meeting_v2",
            "purge_expired_trash_now",
            "delete_transcript_segments",
            "delete_transcript_span_secure",
            "search_meeting_transcript",
            "list_meeting_folders",
            "create_meeting_folder",
            "rename_meeting_folder",
            "delete_meeting_folder",
            "set_meeting_folder",
            "list_meeting_templates",
            "create_meeting_template",
            "update_meeting_template",
            "delete_meeting_template",
            "set_folder_last_template",
            "ask_meetings_v2",
            "start_ask_by_voice",
            "cancel_ask_by_voice",
            "finish_ask_by_voice",
            "ask_meetings_by_voice",
            "count_importable_notes",
            "import_meeting_notes_batch",
            "set_meeting_offer_visible",
        ]
    }
    #[cfg(not(feature = "meetings"))]
    {
        &[]
    }
}

#[cfg(debug_assertions)]
macro_rules! collect_with_debug_commands {
    ($($($seg:ident)::+),* $(,)?) => {
        collect_commands![
            $($($seg)::+,)*
            actions::debug_set_fail_next_paste
        ]
    };
}

#[cfg(not(debug_assertions))]
macro_rules! collect_with_debug_commands {
    ($($($seg:ident)::+),* $(,)?) => {
        collect_commands![$($($seg)::+),*]
    };
}

#[cfg(feature = "speaker-split")]
macro_rules! collect_app_commands {
    ($($($seg:ident)::+),* $(,)?) => {
        collect_with_debug_commands![
            $($($seg)::+,)*
            shortcut::change_meetings_enabled_setting,
            shortcut::change_meeting_language_setting,
            shortcut::set_meeting_language_override_cmd,
            shortcut::meeting_language_plan,
            meeting_controller::open_meeting_consent,
            meeting_controller::confirm_meeting_start,
            meeting_controller::confirm_meeting_start_mic_only,
            meeting_controller::cancel_meeting_consent,
            meeting_controller::stop_meeting,
            meeting_controller::pause_meeting_cmd,
            meeting_controller::resume_meeting_cmd,
            meeting_controller::request_append_meeting_consent,
            meeting_controller::confirm_append_meeting_audio,
            meetings::current_meeting_offer,
            meetings::dismiss_meeting_offer,
            meeting_controller::meeting_status,
            meeting_controller::meeting_segments,
            meetings_list::list_meetings,
            meetings_list::search_meetings,
            meetings_list::export_meeting_markdown,
            meetings_list::delete_meeting,
            meeting_pipeline::rename_meeting,
            meeting_retention::meeting_retention_state,
            meeting_retention::change_delete_audio_after_notes_setting,
            meeting_retention::request_delete_all_meetings,
            meeting_retention::delete_all_meetings,
            meeting_retention::change_notes_markdown_folder_setting,
            v020_commands::meetings_ext::meeting_notes_snapshot,
            v020_commands::meetings_ext::save_meeting_notepad,
            v020_commands::meetings_ext::retry_meeting_notes,
            v020_commands::meetings_ext::catch_up_meeting,
            v020_commands::meetings_ext::pause_meeting,
            v020_commands::meetings_ext::resume_meeting,
            v020_commands::meetings_ext::append_meeting_audio,
            v020_commands::meetings_ext::list_trashed_meetings,
            v020_commands::meetings_ext::restore_trashed_meeting,
            v020_commands::meetings_ext::delete_transcript_span,
            v020_commands::meetings_ext::change_transcript_delete_days_setting,
            v020_commands::meetings_ext::import_meeting_notes,
            v020_commands::meetings_ext::ask_meetings,
            v020_commands::meetings_ext::list_meeting_todos,
            v020_commands::meetings_ext::complete_meeting_todo,
            v020_commands::meetings_ext::agent_access_state,
            v020_commands::meetings_ext::set_agent_access_enabled,
            v020_commands::meetings_ext::set_agent_access_scope,
            v020_commands::meetings_ext::mcp_command_line,
            v020_commands::meetings_ext::get_meeting_custom_words,
            v020_commands::meetings_ext::set_meeting_custom_words,
            calendar_commands::list_calendar_choices,
            calendar_commands::change_selected_calendars_setting,
            calendar_commands::mark_calendar_as_work_setting,
            calendar_commands::list_upcoming_meetings,
            calendar_commands::keep_upcoming_for_notes,
            calendar_commands::close_upcoming_meetings,
            meeting_extras_commands::trash_meeting,
            meeting_extras_commands::delete_meeting_forever,
            meeting_extras_commands::list_trashed_meetings_v2,
            meeting_extras_commands::restore_trashed_meeting_v2,
            meeting_extras_commands::purge_expired_trash_now,
            meeting_extras_commands::delete_transcript_segments,
            meeting_extras_commands::delete_transcript_span_secure,
            meeting_extras_commands::search_meeting_transcript,
            meeting_extras_commands::list_meeting_folders,
            meeting_extras_commands::create_meeting_folder,
            meeting_extras_commands::rename_meeting_folder,
            meeting_extras_commands::delete_meeting_folder,
            meeting_extras_commands::set_meeting_folder,
            meeting_extras_commands::list_meeting_templates,
            meeting_extras_commands::create_meeting_template,
            meeting_extras_commands::update_meeting_template,
            meeting_extras_commands::delete_meeting_template,
            meeting_extras_commands::set_folder_last_template,
            meeting_extras_commands::ask_meetings_v2,
            meeting_extras_commands::start_ask_by_voice,
            meeting_extras_commands::cancel_ask_by_voice,
            meeting_extras_commands::finish_ask_by_voice,
            meeting_extras_commands::ask_meetings_by_voice,
            meeting_extras_commands::count_importable_notes,
            meeting_extras_commands::import_meeting_notes_batch,
            meeting_speakers::list_meeting_speakers,
            meeting_speakers::rename_meeting_speaker_slot,
            meeting_offer_panel::set_meeting_offer_visible
        ]
    };
}

#[cfg(all(feature = "meetings", not(feature = "speaker-split")))]
macro_rules! collect_app_commands {
    ($($($seg:ident)::+),* $(,)?) => {
        collect_with_debug_commands![
            $($($seg)::+,)*
            shortcut::change_meetings_enabled_setting,
            shortcut::change_meeting_language_setting,
            shortcut::set_meeting_language_override_cmd,
            shortcut::meeting_language_plan,
            meeting_controller::open_meeting_consent,
            meeting_controller::confirm_meeting_start,
            meeting_controller::confirm_meeting_start_mic_only,
            meeting_controller::cancel_meeting_consent,
            meeting_controller::stop_meeting,
            meeting_controller::pause_meeting_cmd,
            meeting_controller::resume_meeting_cmd,
            meeting_controller::request_append_meeting_consent,
            meeting_controller::confirm_append_meeting_audio,
            meetings::current_meeting_offer,
            meetings::dismiss_meeting_offer,
            meeting_controller::meeting_status,
            meeting_controller::meeting_segments,
            meetings_list::list_meetings,
            meetings_list::search_meetings,
            meetings_list::export_meeting_markdown,
            meetings_list::delete_meeting,
            meeting_pipeline::rename_meeting,
            meeting_retention::meeting_retention_state,
            meeting_retention::change_delete_audio_after_notes_setting,
            meeting_retention::request_delete_all_meetings,
            meeting_retention::delete_all_meetings,
            meeting_retention::change_notes_markdown_folder_setting,
            v020_commands::meetings_ext::meeting_notes_snapshot,
            v020_commands::meetings_ext::save_meeting_notepad,
            v020_commands::meetings_ext::retry_meeting_notes,
            v020_commands::meetings_ext::catch_up_meeting,
            v020_commands::meetings_ext::pause_meeting,
            v020_commands::meetings_ext::resume_meeting,
            v020_commands::meetings_ext::append_meeting_audio,
            v020_commands::meetings_ext::list_trashed_meetings,
            v020_commands::meetings_ext::restore_trashed_meeting,
            v020_commands::meetings_ext::delete_transcript_span,
            v020_commands::meetings_ext::change_transcript_delete_days_setting,
            v020_commands::meetings_ext::import_meeting_notes,
            v020_commands::meetings_ext::ask_meetings,
            v020_commands::meetings_ext::list_meeting_todos,
            v020_commands::meetings_ext::complete_meeting_todo,
            v020_commands::meetings_ext::agent_access_state,
            v020_commands::meetings_ext::set_agent_access_enabled,
            v020_commands::meetings_ext::set_agent_access_scope,
            v020_commands::meetings_ext::mcp_command_line,
            v020_commands::meetings_ext::get_meeting_custom_words,
            v020_commands::meetings_ext::set_meeting_custom_words,
            calendar_commands::list_calendar_choices,
            calendar_commands::change_selected_calendars_setting,
            calendar_commands::mark_calendar_as_work_setting,
            calendar_commands::list_upcoming_meetings,
            calendar_commands::keep_upcoming_for_notes,
            calendar_commands::close_upcoming_meetings,
            meeting_extras_commands::trash_meeting,
            meeting_extras_commands::delete_meeting_forever,
            meeting_extras_commands::list_trashed_meetings_v2,
            meeting_extras_commands::restore_trashed_meeting_v2,
            meeting_extras_commands::purge_expired_trash_now,
            meeting_extras_commands::delete_transcript_segments,
            meeting_extras_commands::delete_transcript_span_secure,
            meeting_extras_commands::search_meeting_transcript,
            meeting_extras_commands::list_meeting_folders,
            meeting_extras_commands::create_meeting_folder,
            meeting_extras_commands::rename_meeting_folder,
            meeting_extras_commands::delete_meeting_folder,
            meeting_extras_commands::set_meeting_folder,
            meeting_extras_commands::list_meeting_templates,
            meeting_extras_commands::create_meeting_template,
            meeting_extras_commands::update_meeting_template,
            meeting_extras_commands::delete_meeting_template,
            meeting_extras_commands::set_folder_last_template,
            meeting_extras_commands::ask_meetings_v2,
            meeting_extras_commands::start_ask_by_voice,
            meeting_extras_commands::cancel_ask_by_voice,
            meeting_extras_commands::finish_ask_by_voice,
            meeting_extras_commands::ask_meetings_by_voice,
            meeting_extras_commands::count_importable_notes,
            meeting_extras_commands::import_meeting_notes_batch,
            meeting_offer_panel::set_meeting_offer_visible
        ]
    };
}

#[cfg(not(feature = "meetings"))]
macro_rules! collect_app_commands {
    ($($($seg:ident)::+),* $(,)?) => {
        collect_with_debug_commands![$($($seg)::+),*]
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run(cli_args: CliArgs) {
    if cli_args.mcp_ask_user {
        std::process::exit(agent_ask::run_stdio(None));
    }
    if matches!(cli_args.command, Some(crate::cli::CliCommand::Mcp)) {
        std::process::exit(mcp::run_cli());
    }

    // --cleanup-eval scores the on-device cleanup model and exits before any
    // window, tray, or single-instance plugin exists: that plugin would hand
    // the flag to a running app and show its window.
    if let Some(set) = &cli_args.cleanup_eval {
        std::process::exit(cleanup_eval::run_eval(set, &cli_args));
    }

    // Detect portable mode before anything else
    portable::init();

    // Parse console logging directives from RUST_LOG, falling back to info-level logging
    // when the variable is unset
    let console_filter = build_console_filter();

    let specta_builder = Builder::<tauri::Wry>::new()
        .commands(collect_app_commands![
            shortcut::change_binding,
            shortcut::reset_binding,
            shortcut::change_ptt_setting,
            shortcut::change_double_tap_hands_free_setting,
            shortcut::change_audio_feedback_setting,
            shortcut::change_audio_feedback_volume_setting,
            shortcut::change_sound_theme_setting,
            shortcut::change_theme_setting,
            shortcut::change_start_hidden_setting,
            shortcut::change_autostart_setting,
            shortcut::change_translate_to_english_setting,
            shortcut::change_selected_language_setting,
            shortcut::change_overlay_position_setting,
            shortcut::change_overlay_style_setting,
            shortcut::change_debug_mode_setting,
            shortcut::change_word_correction_threshold_setting,
            shortcut::change_extra_recording_buffer_setting,
            shortcut::change_paste_delay_ms_setting,
            shortcut::change_paste_delay_after_ms_setting,
            shortcut::change_paste_method_setting,
            shortcut::get_available_typing_tools,
            shortcut::change_typing_tool_setting,
            shortcut::change_external_script_path_setting,
            shortcut::change_clipboard_handling_setting,
            shortcut::dismiss_clipboard_default_notice,
            shortcut::change_auto_submit_setting,
            shortcut::change_auto_submit_key_setting,
            shortcut::change_post_process_enabled_setting,
            shortcut::change_iterative_correction_setting,
            shortcut::change_experimental_enabled_setting,
            shortcut::change_post_process_base_url_setting,
            shortcut::change_post_process_api_key_setting,
            shortcut::change_post_process_model_setting,
            shortcut::set_post_process_provider,
            shortcut::fetch_post_process_models,
            shortcut::add_post_process_prompt,
            shortcut::update_post_process_prompt,
            shortcut::delete_post_process_prompt,
            shortcut::set_post_process_selected_prompt,
            shortcut::update_custom_words,
            shortcut::update_corrections,
            shortcut::suspend_binding,
            shortcut::resume_binding,
            shortcut::change_mute_while_recording_setting,
            shortcut::change_append_trailing_space_setting,
            shortcut::change_lazy_stream_close_setting,
            shortcut::change_vad_enabled_setting,
            shortcut::change_app_language_setting,
            shortcut::change_update_checks_setting,
            shortcut::change_show_whats_new_on_update_setting,
            shortcut::change_whats_new_last_seen_version_setting,
            shortcut::change_keyboard_implementation_setting,
            shortcut::get_keyboard_implementation,
            shortcut::change_show_tray_icon_setting,
            shortcut::change_transcribe_accelerator_setting,
            shortcut::change_ort_accelerator_setting,
            shortcut::change_transcribe_gpu_device,
            shortcut::get_available_accelerators,
            shortcut::coco_keys::start_handy_keys_recording,
            shortcut::coco_keys::stop_handy_keys_recording,
            trigger_update_check,
            show_main_window_command,
            commands::cancel_operation,
            commands::is_portable,
            commands::get_app_dir_path,
            commands::get_app_settings,
            commands::get_default_settings,
            commands::get_log_dir_path,
            commands::set_log_level,
            commands::open_recordings_folder,
            commands::open_log_dir,
            commands::open_app_data_dir,
            commands::check_apple_intelligence_available,
            commands::initialize_enigo,
            commands::initialize_shortcuts,
            commands::models::get_available_models,
            commands::models::get_model_info,
            commands::models::download_model,
            commands::models::delete_model,
            commands::models::cancel_download,
            commands::models::set_active_model,
            commands::models::get_current_model,
            commands::models::get_transcription_model_status,
            commands::models::is_model_loading,
            commands::models::rescan_local_models,
            commands::audio::update_microphone_mode,
            commands::audio::get_microphone_mode,
            commands::audio::get_windows_microphone_permission_status,
            commands::audio::open_microphone_privacy_settings,
            commands::audio::open_system_audio_privacy_settings,
            commands::audio::get_available_microphones,
            commands::audio::set_selected_microphone,
            commands::audio::get_selected_microphone,
            commands::audio::get_available_output_devices,
            commands::audio::set_selected_output_device,
            commands::audio::get_selected_output_device,
            commands::audio::play_test_sound,
            commands::audio::check_custom_sounds,
            commands::audio::set_clamshell_microphone,
            commands::audio::get_clamshell_microphone,
            commands::audio::is_recording,
            commands::audio::set_microphone_priority,
            commands::audio::get_microphone_priority,
            commands::transcription::set_model_unload_timeout,
            commands::transcription::get_model_load_status,
            commands::transcription::unload_model_manually,
            commands::history::get_history_entries,
            commands::history::toggle_history_entry_saved,
            commands::history::get_audio_file_path,
            commands::history::delete_history_entry,
            commands::history::retry_history_entry_transcription,
            commands::history::update_history_limit,
            commands::history::update_history_entry_text,
            commands::history::update_recording_retention_period,
            commands::history::update_keep_dictations,
            commands::history::update_keep_dictation_audio,
            commands::history::change_never_store_history_setting,
            commands::history::change_keep_dictation_audio_setting,
            helpers::clamshell::is_laptop,
            app_profiles::get_active_app_info,
            app_profiles::app_display_names,
            app_profiles::update_app_profiles,
            app_profiles::update_blocked_apps,
            v020_commands::update_saved_blocks,
            v020_commands::set_read_focused_text_enabled,
            v020_commands::set_read_focused_window_context_enabled,
            v020_commands::remember_spelling_fix,
            v020_commands::change_offer_remember_fix_setting,
            v020_commands::change_repo_words_setting,
            v020_commands::change_filler_word_removal_setting,
            v020_commands::change_agents_can_ask_setting,
            v020_commands::update_blocked_sites,
            v020_commands::dismiss_fix_offer,
            v020_commands::check_accessibility_stale,
            v020_commands::home_health_status,
            v020_commands::evaluate_try_it_take,
            v020_commands::evaluate_update_notice_and_advance,
        ])
        .events(collect_events![
            managers::history::HistoryUpdatePayload,
            managers::transcription::StreamTextEvent,
            managers::transcription::StreamPhaseEvent,
            cleanup_resolver::CleanupStatus,
            actions::CleanupProgress,
        ])
        .typ::<managers::model::ModelDownloadFailed>();

    #[cfg(debug_assertions)] // <- Only export on non-release builds
    {
        // A meetings build writes its own bindings file so `src/bindings.ts` stays the default build's.
        #[cfg(feature = "meetings")]
        let bindings_path = "../src/bindings.meetings.ts";
        #[cfg(not(feature = "meetings"))]
        let bindings_path = "../src/bindings.ts";
        specta_builder
            .export(
                Typescript::default().bigint(BigIntExportBehavior::Number),
                bindings_path,
            )
            .expect("Failed to export typescript bindings");
    }

    let invoke_handler = specta_builder.invoke_handler();

    // The headless path must run as its own instance (see the single-instance
    // note below), not forward to an already-running app.
    let headless_mode = cli_args.transcribe_file.is_some()
        || cli_args.list_devices
        || cli_args.list_models
        || cli_args.print_app_data_dir;

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .device_event_filter(tauri::DeviceEventFilter::Always)
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            LogBuilder::new()
                .level(log::LevelFilter::Trace) // Set to most verbose level globally
                .max_file_size(500_000)
                .rotation_strategy(RotationStrategy::KeepOne)
                .clear_targets()
                .targets([
                    // Console output respects RUST_LOG environment variable. In
                    // headless mode (--transcribe-file/--list-devices/--list-models)
                    // stdout carries only the result (JSON or plain), so send console
                    // logs to stderr instead to keep stdout clean for CI parsing.
                    Target::new(if headless_mode {
                        TargetKind::Stderr
                    } else {
                        TargetKind::Stdout
                    })
                    .filter({
                        let console_filter = console_filter.clone();
                        move |metadata| console_filter.enabled(metadata)
                    }),
                    // File logs respect the user's settings (stored in FILE_LOG_LEVEL atomic)
                    Target::new(if let Some(data_dir) = portable::data_dir() {
                        TargetKind::Folder {
                            path: data_dir.join("logs"),
                            file_name: Some("coco-voice".into()),
                        }
                    } else {
                        TargetKind::LogDir {
                            file_name: Some("coco-voice".into()),
                        }
                    })
                    .filter(|metadata| {
                        let file_level = FILE_LOG_LEVEL.load(Ordering::Relaxed);
                        metadata.level() <= level_filter_from_u8(file_level)
                    }),
                    // Stream logs to the webview (via the `log://log` event) so the
                    // debug panel's live log viewer can show them in real time. Only
                    // active while debug mode is on (its sole consumer), and shares the
                    // file log level so the "Log Level" setting controls verbosity.
                    Target::new(TargetKind::Webview).filter(|metadata| {
                        WEBVIEW_LOG_STREAMING.load(Ordering::Relaxed)
                            && metadata.level()
                                <= level_filter_from_u8(FILE_LOG_LEVEL.load(Ordering::Relaxed))
                    }),
                ])
                .build(),
        );

    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(tauri_nspanel::init());
    }

    // Single-instance forwards CLI args to an already-running Coco Voice and exits.
    // That would make the headless path
    // (--transcribe-file/--list-devices/--list-models) a silent no-op whenever the
    // app is already open, so skip it in headless mode and run a standalone
    // instance instead.
    if !headless_mode {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|a| a == "--toggle-transcription") {
                signal_handle::send_transcription_input(app, "transcribe", "CLI");
            } else if args.iter().any(|a| a == "--toggle-post-process") {
                signal_handle::send_transcription_input(app, "transcribe_with_post_process", "CLI");
            } else if args.iter().any(|a| a == "--cancel") {
                crate::utils::cancel_current_operation(app);
            } else {
                show_main_window(app);
            }
        }));
    }

    builder
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_macos_permissions::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(cli_args.clone())
        .setup(move |app| {
            // Setup runs on the UI thread. Paste guards compare against this id.
            input::record_ui_thread();

            specta_builder.mount_events(app);

            // Before the headless return, so both GUI and one-shot runs wipe once.
            // Persisted here, before the `--debug` Trace override, which stays in memory.
            clear_legacy_transcript_logs(app.handle());
            for dir in [
                portable::app_data_dir(app.handle()),
                portable::app_log_dir(app.handle()),
            ] {
                let dir = match dir {
                    Ok(dir) => dir,
                    Err(e) => {
                        log::warn!("Could not resolve a folder to make private: {e}");
                        continue;
                    }
                };
                if let Err(e) = portable::make_private(&dir) {
                    log::warn!("Could not make {} private to this user: {e}", dir.display());
                }
            }

            // Headless one-shot path (`--transcribe-file` / `--list-devices` /
            // `--list-models`): initialize only what transcription needs — the
            // store/paths plugins, the model + transcription managers, and the
            // transcribe-cpp backend + accelerator settings — then run on a worker
            // thread and exit. Deliberately skips the window, tray, overlay, audio
            // recorder (so it never opens the mic, even with always_on_microphone),
            // signal handlers, and autostart that initialize_core_logic sets up.
            if cli_args.print_app_data_dir {
                let resolved =
                    portable::app_data_dir(app.handle()).expect("resolve Tauri app_data_dir");
                println!("identifier={}", app.config().identifier);
                println!("app_data_dir={}", resolved.display());
                std::process::exit(0);
            }

            if headless_mode {
                let app_handle = app.handle().clone();
                let model_manager = Arc::new(
                    ModelManager::new(&app_handle).expect("Failed to initialize model manager"),
                );
                let transcription_manager = Arc::new(
                    TranscriptionManager::new(&app_handle, model_manager.clone())
                        .expect("Failed to initialize transcription manager"),
                );
                app_handle.manage(model_manager);
                app_handle.manage(transcription_manager);
                managers::transcription::init_transcribe_backend();
                managers::transcription::apply_accelerator_settings(&app_handle);

                let handle = app_handle.clone();
                let args = cli_args.clone();
                std::thread::spawn(move || {
                    let code = run_headless_transcription(&handle, &args);
                    // Drop the loaded engine before teardown: ggml-metal's global
                    // device free asserts (SIGABRT) if a model's Metal resources
                    // are still alive at C++ static-destructor time.
                    if let Some(tm) = handle.try_state::<Arc<TranscriptionManager>>() {
                        let _ = tm.unload_model();
                    }
                    // process::exit (not app.exit, which exits 0 regardless) so the
                    // exit code propagates to the shell for CI gating. Flush first
                    // since process::exit runs no destructors / buffer flushes.
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    let _ = std::io::stderr().flush();
                    std::process::exit(code);
                });
                return Ok(());
            }

            #[cfg(feature = "meetings")]
            meetings::install(app.handle());

            // Create main window programmatically so we can set data_directory
            // for portable mode (redirects WebView2 cache to portable Data dir)
            let mut win_builder =
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("/".into()))
                    .title("Coco Voice")
                    .inner_size(960.0, 640.0)
                    .min_inner_size(680.0, 570.0)
                    .resizable(true)
                    .maximizable(false)
                    .visible(false);

            if let Some(data_dir) = portable::data_dir() {
                win_builder = win_builder.data_directory(data_dir.join("webview"));
            }

            win_builder.build()?;

            let mut settings = get_settings(app.handle());

            // Apply the persisted appearance theme to the Windows title bar before
            // the window is shown, so it matches the in-app palette without a flash
            // of the wrong theme. On macOS/Linux, Tauri themes are app-wide and
            // would also affect windows that intentionally keep the system theme.
            #[cfg(target_os = "windows")]
            shortcut::apply_window_theme(app.handle(), settings.theme);

            // CLI --debug flag overrides debug_mode and log level (runtime-only, not persisted)
            if cli_args.debug {
                settings.debug_mode = true;
                settings.log_level = settings::LogLevel::Trace;
            }

            let tauri_log_level: tauri_plugin_log::LogLevel = settings.log_level.into();
            let file_log_level: log::Level = tauri_log_level.into();
            // Store the file log level in the atomic for the filter to use
            FILE_LOG_LEVEL.store(file_log_level.to_level_filter() as u8, Ordering::Relaxed);
            // Only forward logs to the webview while debug mode is on (the live log
            // viewer is the sole consumer and only exists in debug mode). This also
            // honors the runtime `--debug` override applied to `settings` above.
            WEBVIEW_LOG_STREAMING.store(settings.debug_mode, Ordering::Relaxed);
            let app_handle = app.handle().clone();
            app.manage(TranscriptionCoordinator::new(app_handle.clone()));
            #[cfg(feature = "meetings")]
            app.manage(calendar_commands::CalendarState::for_bundle_id(
                &app.config().identifier,
            ));

            initialize_core_logic(&app_handle);
            // Now the managers exist: run any toggle that arrived meanwhile (#212).
            app_handle.state::<TranscriptionCoordinator>().mark_ready();

            // Populate the overlay-enabled cache from initial settings so the
            // audio path (overlay::emit_levels, called ~24 Hz during recording)
            // can do a single atomic load instead of reading the Tauri store.
            // Kept in sync by shortcut::change_overlay_style_setting.
            overlay::update_overlay_enabled_cache(
                settings.overlay_style != settings::OverlayStyle::None,
            );

            // Pre-warm GPU/accelerator enumeration on a background thread. The first
            // get_available_accelerators call enumerates ORT execution providers and
            // transcribe-cpp compute devices, which can take a moment; without this
            // the cost is paid synchronously when the user first opens Advanced
            // settings, freezing the UI. Result is cached in a OnceLock.
            std::thread::spawn(|| {
                let _ = crate::managers::transcription::get_available_accelerators();
            });

            // Hide tray icon if --no-tray was passed
            if cli_args.no_tray {
                tray::set_tray_visibility(&app_handle, false);
            }

            // Show main window only if not starting hidden.
            // CLI --start-hidden flag overrides the setting.
            // But if permission onboarding is required, always show the window.
            let should_hide = settings.start_hidden || cli_args.start_hidden;
            let should_force_show = should_force_show_permissions_window(&app_handle);

            // If start_hidden but tray is disabled, we must show the window
            // anyway. Without a tray icon, the dock is the only way back in.
            let tray_available = settings.show_tray_icon && !cli_args.no_tray;
            if should_force_show || !should_hide || !tray_available {
                show_main_window(&app_handle);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _res = window.hide();

                #[cfg(target_os = "macos")]
                {
                    let settings = get_settings(window.app_handle());
                    let tray_visible =
                        settings.show_tray_icon && !window.app_handle().state::<CliArgs>().no_tray;
                    if tray_visible {
                        // Tray is available: hide the dock icon, app lives in the tray
                        let res = window
                            .app_handle()
                            .set_activation_policy(tauri::ActivationPolicy::Accessory);
                        if let Err(e) = res {
                            log::error!("Failed to set activation policy: {}", e);
                        }
                    }
                    // No tray: keep the dock icon visible so the user can reopen
                }
            }
            tauri::WindowEvent::ThemeChanged(theme) => {
                log::info!("Theme changed to: {:?}", theme);
                // Re-apply the current tray state with the new theme's icon set
                utils::refresh_tray_icon(window.app_handle());
            }
            _ => {}
        })
        .invoke_handler(invoke_handler)
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match &event {
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                show_main_window(app);
            }
            // Teardown transcribe.cpp before exit
            tauri::RunEvent::Exit => {
                #[cfg(feature = "meetings")]
                meetings::stop_offer_poll();
                if let Some(tm) = app.try_state::<Arc<TranscriptionManager>>() {
                    let _ = tm.unload_model();
                }
            }
            _ => {}
        });
}

#[cfg(test)]
mod meetings_gate_tests {
    use super::meetings_command_names;

    #[test]
    fn meetings_commands_gated_by_feature() {
        let names = meetings_command_names();
        #[cfg(not(feature = "meetings"))]
        assert!(
            names.is_empty(),
            "a build without the meetings feature registers no meetings commands"
        );
        #[cfg(feature = "meetings")]
        {
            assert!(
                !names.is_empty(),
                "a meetings build registers the meetings setting command"
            );
            assert!(names.contains(&"change_meetings_enabled_setting"));
            assert!(names.contains(&"current_meeting_offer"));
            assert!(names.contains(&"dismiss_meeting_offer"));
            assert!(names.contains(&"meeting_status"));
            assert!(names.contains(&"confirm_meeting_start_mic_only"));
            assert!(names.contains(&"meeting_segments"));
            assert!(names.contains(&"start_ask_by_voice"));
            assert!(names.contains(&"cancel_ask_by_voice"));
            assert!(names.contains(&"finish_ask_by_voice"));
            assert!(names.contains(&"ask_meetings_by_voice"));
            #[cfg(not(feature = "speaker-split"))]
            {
                assert!(!names.contains(&"list_meeting_speakers"));
                assert!(!names.contains(&"rename_meeting_speaker_slot"));
            }
            #[cfg(feature = "speaker-split")]
            {
                assert!(names.contains(&"list_meeting_speakers"));
                assert!(names.contains(&"rename_meeting_speaker_slot"));
            }
        }
    }
}

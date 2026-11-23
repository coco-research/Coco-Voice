//! `Y7` Do-Not-Copy Privacy Guards (`D61`, `D62`, `D63`, `D72`, `D73`).
//!
//! Verifies at test time that Coco Voice:
//! 1. Collects no usage telemetry or word counts (`D63`).
//! 2. Never uploads user audio or feedback carrying audio (`D62`).
//! 3. Sends nothing off the Mac except model downloads, the update check, and
//!    an opt-in cloud clean-up provider configured by the user (`D61`, `D72`, `D73`).

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if matches!(name, "node_modules" | "target" | "dist" | ".git") {
                continue;
            }
        }
        if path.is_dir() {
            collect_source_files(&path, out);
        } else if matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("rs" | "ts" | "tsx")
        ) {
            out.push(path);
        }
    }
}

fn production_code_of(path: &Path, raw: &str) -> String {
    if path.extension().and_then(|s| s.to_str()) == Some("rs") {
        if let Some(idx) = raw.find("#[cfg(test)]") {
            return raw[..idx].to_string();
        }
    }
    raw.to_string()
}

#[test]
fn d63_voice_collects_no_usage_or_word_counts() {
    let root = repo_root();

    // 1. AppSettings carries zero telemetry, analytics, or word-count fields.
    let defaults = serde_json::to_value(crate::settings::get_default_settings())
        .expect("serialize AppSettings");
    let obj = defaults.as_object().expect("AppSettings object");
    for forbidden_key in [
        "telemetry",
        "analytics",
        "usage_stats",
        "word_count",
        "words_dictated",
        "total_words",
        "session_count",
        "amplitude",
        "posthog",
        "mixpanel",
        "sentry",
    ] {
        assert!(
            !obj.keys().any(|k| k.contains(forbidden_key)),
            "AppSettings must not contain usage/word-count key matching `{forbidden_key}`"
        );
    }

    // 2. Local SQLite schemas (history.rs + meetings.rs) store no telemetry or cumulative word-count tables.
    for schema_rel in [
        "src-tauri/src/managers/history.rs",
        "src-tauri/src/managers/meetings.rs",
    ] {
        let sql_src = fs::read_to_string(root.join(schema_rel)).expect("read schema source");
        for banned_sql in [
            "word_count",
            "usage_metrics",
            "telemetry",
            "analytics_events",
            "words_per_minute_telemetry",
        ] {
            assert!(
                !sql_src.to_ascii_lowercase().contains(banned_sql),
                "{schema_rel} must not define `{banned_sql}`"
            );
        }
    }

    // 3. Zero analytics SDKs in Cargo.toml or package.json.
    let cargo_toml =
        fs::read_to_string(root.join("src-tauri/Cargo.toml")).expect("read Cargo.toml");
    let pkg_json = fs::read_to_string(root.join("package.json")).expect("read package.json");
    for sdk in [
        "posthog",
        "mixpanel",
        "amplitude",
        "@segment/",
        "segment-analytics",
        "analytics-node",
        "sentry",
        "opentelemetry",
        "google-analytics",
    ] {
        assert!(
            !cargo_toml.to_ascii_lowercase().contains(sdk),
            "Cargo.toml must not depend on analytics SDK `{sdk}`"
        );
        assert!(
            !pkg_json.to_ascii_lowercase().contains(sdk),
            "package.json must not depend on analytics SDK `{sdk}`"
        );
    }
}

#[test]
fn d62_voice_never_uploads_audio_or_feedback_carrying_audio() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_source_files(&root.join("src-tauri/src"), &mut files);
    collect_source_files(&root.join("src"), &mut files);

    for file in files {
        if file.ends_with("y7_privacy_guards.rs") {
            continue;
        }
        let raw = fs::read_to_string(&file).expect("read source");
        let prod = production_code_of(&file, &raw);
        for banned in [
            "upload_audio",
            "uploadAudio",
            "audio_upload",
            "feedback_audio",
            "send_feedback_with_audio",
            "multipart::Form",
            "audio/wav",
        ] {
            if banned == "audio/wav" && file.ends_with("HistorySettings.tsx") {
                // Local Blob URL playback inside the webview (`new Blob([fileData], { type: "audio/wav" })`), never uploaded.
                continue;
            }
            assert!(
                !prod.contains(banned),
                "D62 violation: {} contains forbidden audio/feedback upload marker `{banned}`",
                file.display()
            );
        }
    }
}

#[test]
fn d61_d72_d73_nothing_leaves_mac_except_models_update_check_and_opt_in_user_key_cloud_cleanup() {
    let root = repo_root();
    let mut rust_files = Vec::new();
    collect_source_files(&root.join("src-tauri/src"), &mut rust_files);

    // Every Rust file that touches `reqwest` in production code must belong strictly to:
    // - `managers/model.rs` (model downloads)
    // - `llm_client.rs` (user-key cloud clean-up when explicitly enabled by the user)
    for file in &rust_files {
        if file.ends_with("y7_privacy_guards.rs") {
            continue;
        }
        let raw = fs::read_to_string(file).expect("read rust source");
        let prod = production_code_of(file, &raw);
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        if prod.contains("reqwest::") {
            assert!(
                rel == "src-tauri/src/managers/model.rs" || rel == "src-tauri/src/llm_client.rs",
                "Unexpected outbound HTTP client (`reqwest`) in `{rel}`; only model downloads and opt-in user-key cloud clean-up may use reqwest"
            );
        }

        // D61 (no remote health/terms kill-switch), D72 (no vendor 30-day cloud copy), D73 (no US/EU cloud storage residency).
        for forbidden in [
            "remote_kill_switch",
            "health_agreement_gate",
            "cloud_retention_days",
            "data_residency_region",
            "sync_transcript_to_cloud",
        ] {
            assert!(
                !prod.contains(forbidden),
                "D61/D72/D73 violation in `{rel}`: `{forbidden}`"
            );
        }
    }

    // Focused-box formatting (`focused_text.rs`) must never import reqwest or llm_client.
    let focused_src =
        fs::read_to_string(root.join("src-tauri/src/focused_text.rs")).expect("read focused_text");
    let focused_prod = production_code_of(Path::new("focused_text.rs"), &focused_src);
    assert!(!focused_prod.contains("reqwest"));
    assert!(!focused_prod.contains("llm_client"));
    assert!(!focused_prod.contains("HistoryManager"));
}

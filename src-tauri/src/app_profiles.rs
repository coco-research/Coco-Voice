//! Shared frontmost-application lookup and per-app profile helpers.
//!
//! Resolves the active application's display name, bundle identifier, and
//! executable stem in-process without reading any screen text or window title.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
pub struct ActiveAppInfo {
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub process_stem: Option<String>,
}

pub fn is_self_identifier(value: &str) -> bool {
    let t = value.trim();
    t.eq_ignore_ascii_case("Coco Voice")
        || t.eq_ignore_ascii_case("coco-voice")
        || t.eq_ignore_ascii_case("com.cocoresearch.cocovoice")
}

pub fn is_self_app(info: &ActiveAppInfo) -> bool {
    is_self_identifier(&info.app_name)
        || info.bundle_id.as_deref().is_some_and(is_self_identifier)
        || info.process_stem.as_deref().is_some_and(is_self_identifier)
}

pub fn filter_active_app(mut info: ActiveAppInfo) -> Option<ActiveAppInfo> {
    info.app_name = info.app_name.trim().to_string();
    info.bundle_id = info
        .bundle_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    info.process_stem = info
        .process_stem
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if info.app_name.is_empty() {
        info.app_name = info.process_stem.clone().unwrap_or_default();
    }
    if (info.app_name.is_empty() && info.bundle_id.is_none() && info.process_stem.is_none())
        || is_self_app(&info)
    {
        return None;
    }
    Some(info)
}

// ponytail: ceiling = macOS NSWorkspace frontmostApplication (0 subprocesses, 0 screen/window-title reads); upgrade path = foreground process query on Windows/Linux.
pub fn read_active_app() -> Option<ActiveAppInfo> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        use std::path::Path;

        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let app_name = front
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let bundle_id = front.bundleIdentifier().map(|s| s.to_string());
        let process_stem = front.executableURL().and_then(|u| u.path()).and_then(|p| {
            let s = p.to_string();
            Path::new(&s)
                .file_stem()
                .and_then(|st| st.to_str())
                .map(str::to_string)
        });
        filter_active_app(ActiveAppInfo {
            app_name,
            bundle_id,
            process_stem,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod per_app_tests {
    use super::*;

    #[test]
    fn frontmost_app_filter_excludes_self_identifiers_and_keeps_external_apps() {
        assert_eq!(
            filter_active_app(ActiveAppInfo {
                app_name: "Coco Voice".into(),
                bundle_id: Some("com.cocoresearch.cocovoice".into()),
                process_stem: Some("coco-voice".into()),
            }),
            None
        );
        assert_eq!(
            filter_active_app(ActiveAppInfo {
                app_name: "Custom Build".into(),
                bundle_id: Some("com.cocoresearch.cocovoice".into()),
                process_stem: None,
            }),
            None
        );
        assert_eq!(
            filter_active_app(ActiveAppInfo {
                app_name: "Custom Build".into(),
                bundle_id: None,
                process_stem: Some("coco-voice".into()),
            }),
            None
        );
        let mail = filter_active_app(ActiveAppInfo {
            app_name: " Mail ".into(),
            bundle_id: Some("com.apple.mail".into()),
            process_stem: Some("Mail".into()),
        })
        .expect("external app retained");
        assert_eq!(mail.app_name, "Mail");
        assert_eq!(mail.bundle_id.as_deref(), Some("com.apple.mail"));
    }
}

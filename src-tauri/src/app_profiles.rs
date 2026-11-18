//! Per-app dictation profiles and blocked-app shortcut guard.
//!
//! Resolves frontmost application identity, app-kind tone defaults, per-app
//! overrides, blocked-app shortcut gating, and capitalization and punctuation
//! styles without reading any screen text.

use crate::settings::{AppSettings, CorrectionPair};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::path::Path;

const CHAT_APPS: &str = "com.tinyspeck.slackmacgap|slack|com.apple.mobilesms|messages|com.hnc.discord|discord|ru.keepcoder.telegram|telegram|net.whatsapp.whatsapp|whatsapp|org.whispersystems.signal-desktop|signal";
const EMAIL_APPS: &str = "com.apple.mail|mail|com.microsoft.outlook|microsoft outlook|com.superhuman.electron|superhuman|org.mozilla.thunderbird|thunderbird";
const CODE_APPS: &str = "com.todesktop.230313mzl4w4u92|cursor|com.microsoft.vscode|code|visual studio code|com.apple.dt.xcode|xcode|dev.zed.zed|zed|com.googlecode.iterm2|iterm2|com.apple.terminal|terminal";
const DOC_APPS: &str =
    "notion.id|notion|com.apple.notes|notes|com.apple.iwork.pages|pages|md.obsidian|obsidian";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppTone {
    #[default]
    Default,
    Auto,
    Casual,
    Direct,
    Formal,
    Code,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapitalizationStyle {
    #[default]
    Default,
    Sentence,
    Lowercase,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum PunctuationStyle {
    #[default]
    Default,
    Full,
    DropTrailingPeriod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceStyle {
    pub capitalization: CapitalizationStyle,
    pub punctuation: PunctuationStyle,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Chat,
    Email,
    Code,
    Document,
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
pub struct ActiveAppInfo {
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub process_stem: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type, Default)]
pub struct AppProfile {
    pub id: String,
    pub name: String,
    pub app_identifier: String,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub tone: AppTone,
    #[serde(default)]
    pub capitalization: CapitalizationStyle,
    #[serde(default)]
    pub punctuation: PunctuationStyle,
    #[serde(default)]
    pub post_process_enabled: Option<bool>,
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub corrections: Vec<CorrectionPair>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartDecision {
    Allow,
    BlockedApp(String),
}

impl StartDecision {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Self::BlockedApp(_))
    }
}

pub(crate) struct ResolvedAppProfile {
    pub settings: AppSettings,
    pub surface_style: SurfaceStyle,
    pub post_process_override: Option<bool>,
}

pub fn parse_lsappinfo_info(raw: &str) -> Option<ActiveAppInfo> {
    let (mut bundle_id, mut app_name, mut process_stem) = (None, None, None);
    for line in raw.lines() {
        let Some((k, v)) = line.trim().split_once('=') else {
            continue;
        };
        let val = v.trim();
        if val == "NULL" {
            continue;
        }
        let val = val.trim_matches('"').trim();
        if val.is_empty() {
            continue;
        }
        match k.trim().trim_matches('"') {
            "CFBundleIdentifier" => bundle_id = Some(val.to_string()),
            "LSDisplayName" => app_name = Some(val.to_string()),
            "CFBundleExecutablePath" => {
                process_stem = Path::new(val)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
            }
            _ => {}
        }
    }
    if bundle_id.is_none() && app_name.is_none() && process_stem.is_none() {
        return None;
    }
    Some(ActiveAppInfo {
        app_name: app_name
            .or_else(|| process_stem.clone())
            .unwrap_or_default(),
        bundle_id,
        process_stem,
    })
}

// ponytail: ceiling = macOS lsappinfo bundle/name/executable query (0 screen read); upgrade path = foreground process query on Windows/Linux.
pub fn detect_frontmost_app() -> Option<ActiveAppInfo> {
    #[cfg(target_os = "macos")]
    {
        let front = std::process::Command::new("/usr/bin/lsappinfo")
            .arg("front")
            .output()
            .ok()?;
        let asn = String::from_utf8_lossy(&front.stdout).trim().to_string();
        if !front.status.success() || asn.is_empty() || asn == "NULL" {
            return None;
        }
        let info = std::process::Command::new("/usr/bin/lsappinfo")
            .args(["info", "-only", "bundleid,name,executablepath", &asn])
            .output()
            .ok()?;
        info.status
            .success()
            .then(|| parse_lsappinfo_info(&String::from_utf8_lossy(&info.stdout)))
            .flatten()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
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

pub fn app_profile_matches(saved: &str, info: &ActiveAppInfo) -> bool {
    let p = saved.trim();
    if p.is_empty() || is_self_app(info) || is_self_identifier(p) {
        return false;
    }
    let eq = |c: &str| c.trim().eq_ignore_ascii_case(p);
    info.bundle_id.as_deref().is_some_and(eq)
        || (!info.app_name.is_empty() && eq(&info.app_name))
        || info.process_stem.as_deref().is_some_and(eq)
}

pub(crate) fn check_start_allowed(
    settings: &AppSettings,
    app: Option<&ActiveAppInfo>,
) -> StartDecision {
    let Some(info) = app.filter(|i| !is_self_app(i)) else {
        return StartDecision::Allow;
    };
    if let Some(pat) = settings
        .blocked_apps
        .iter()
        .find(|b| app_profile_matches(b, info))
    {
        return StartDecision::BlockedApp(pat.trim().to_string());
    }
    if let Some(p) = settings
        .app_profiles
        .iter()
        .find(|p| p.blocked && app_profile_matches(&p.app_identifier, info))
    {
        return StartDecision::BlockedApp(p.app_identifier.trim().to_string());
    }
    StartDecision::Allow
}

pub(crate) fn check_active_app_start(settings: &AppSettings) -> StartDecision {
    if settings.blocked_apps.is_empty() && !settings.app_profiles.iter().any(|p| p.blocked) {
        return StartDecision::Allow;
    }
    check_start_allowed(settings, detect_frontmost_app().as_ref())
}

pub fn classify_app_kind(info: &ActiveAppInfo) -> AppKind {
    let fields = [
        info.bundle_id.as_deref().unwrap_or(""),
        info.app_name.as_str(),
        info.process_stem.as_deref().unwrap_or(""),
    ];
    let matches_list = |list: &str| {
        list.split('|')
            .any(|item| fields.iter().any(|f| f.trim().eq_ignore_ascii_case(item)))
    };
    if matches_list(CHAT_APPS) {
        AppKind::Chat
    } else if matches_list(EMAIL_APPS) {
        AppKind::Email
    } else if matches_list(CODE_APPS) {
        AppKind::Code
    } else if matches_list(DOC_APPS) {
        AppKind::Document
    } else {
        AppKind::Other
    }
}

pub fn tone_defaults(tone: AppTone, app: Option<&ActiveAppInfo>) -> SurfaceStyle {
    let effective = match tone {
        AppTone::Auto => match app.map(classify_app_kind).unwrap_or(AppKind::Other) {
            AppKind::Chat => AppTone::Casual,
            AppKind::Email => AppTone::Formal,
            AppKind::Code => AppTone::Code,
            AppKind::Document => AppTone::Direct,
            AppKind::Other => AppTone::Default,
        },
        other => other,
    };
    match effective {
        AppTone::Default | AppTone::Auto => SurfaceStyle::default(),
        AppTone::Casual => SurfaceStyle {
            capitalization: CapitalizationStyle::Lowercase,
            punctuation: PunctuationStyle::DropTrailingPeriod,
        },
        AppTone::Direct | AppTone::Formal => SurfaceStyle {
            capitalization: CapitalizationStyle::Sentence,
            punctuation: PunctuationStyle::Full,
        },
        AppTone::Code => SurfaceStyle {
            capitalization: CapitalizationStyle::Default,
            punctuation: PunctuationStyle::DropTrailingPeriod,
        },
    }
}

fn clamp_str(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

#[allow(dead_code)]
pub(crate) fn normalize_app_profiles(
    profiles: Vec<AppProfile>,
    settings: &AppSettings,
) -> Vec<AppProfile> {
    let prompts: HashSet<&str> = settings
        .post_process_prompts
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    let providers: HashSet<&str> = settings
        .post_process_providers
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    let (mut ids, mut apps, mut out) = (HashSet::new(), HashSet::new(), Vec::new());
    for mut p in profiles {
        if out.len() >= 64 {
            break;
        }
        p.id = clamp_str(&p.id, 128);
        p.name = clamp_str(&p.name, 80);
        p.app_identifier = clamp_str(&p.app_identifier, 260);
        if p.id.is_empty()
            || p.app_identifier.is_empty()
            || is_self_identifier(&p.app_identifier)
            || !ids.insert(p.id.clone())
            || !apps.insert(p.app_identifier.to_ascii_lowercase())
        {
            continue;
        }
        if p.name.is_empty() {
            p.name = p.app_identifier.clone();
        }
        p.prompt_id = p
            .prompt_id
            .map(|s| clamp_str(&s, 128))
            .filter(|s| prompts.contains(s.as_str()));
        p.provider_id = p
            .provider_id
            .map(|s| clamp_str(&s, 128))
            .filter(|s| providers.contains(s.as_str()));
        p.model = p
            .model
            .map(|s| clamp_str(&s, 260))
            .filter(|s| !s.is_empty());
        out.push(p);
    }
    out
}

pub(crate) fn resolve_for_app(
    settings: &AppSettings,
    app: Option<&ActiveAppInfo>,
) -> ResolvedAppProfile {
    let default_out = || ResolvedAppProfile {
        settings: settings.clone(),
        surface_style: SurfaceStyle::default(),
        post_process_override: None,
    };
    let Some(info) = app.filter(|i| !is_self_app(i)) else {
        return default_out();
    };
    let Some(profile) = settings
        .app_profiles
        .iter()
        .find(|p| app_profile_matches(&p.app_identifier, info))
    else {
        return default_out();
    };
    let mut effective = settings.clone();

    if let Some(enabled) = profile.post_process_enabled {
        effective.post_process_enabled = enabled;
        if enabled && effective.post_process_selected_prompt_id.is_none() {
            effective.post_process_selected_prompt_id =
                effective.post_process_prompts.first().map(|p| p.id.clone());
        }
    }
    if let Some(pid) = profile
        .prompt_id
        .as_ref()
        .filter(|id| effective.post_process_prompts.iter().any(|p| &p.id == *id))
    {
        effective.post_process_selected_prompt_id = Some(pid.clone());
    }
    if let Some(prov) = profile
        .provider_id
        .as_ref()
        .filter(|id| effective.post_process_provider(id).is_some())
    {
        effective.post_process_provider_id = prov.clone();
    }
    if let Some(m) = profile.model.as_ref().filter(|m| !m.trim().is_empty()) {
        effective
            .post_process_models
            .insert(effective.post_process_provider_id.clone(), m.clone());
    }
    effective
        .corrections
        .extend(profile.corrections.iter().cloned());

    let base = tone_defaults(profile.tone, Some(info));
    ResolvedAppProfile {
        settings: effective,
        surface_style: SurfaceStyle {
            capitalization: if profile.capitalization == CapitalizationStyle::Default {
                base.capitalization
            } else {
                profile.capitalization
            },
            punctuation: if profile.punctuation == PunctuationStyle::Default {
                base.punctuation
            } else {
                profile.punctuation
            },
        },
        post_process_override: profile.post_process_enabled,
    }
}

pub(crate) fn resolve_active_profile(settings: &AppSettings) -> ResolvedAppProfile {
    if settings.app_profiles.is_empty() {
        return ResolvedAppProfile {
            settings: settings.clone(),
            surface_style: SurfaceStyle::default(),
            post_process_override: None,
        };
    }
    resolve_for_app(settings, detect_frontmost_app().as_ref())
}

pub fn apply_surface_style(text: &str, style: SurfaceStyle) -> String {
    if style == SurfaceStyle::default() || text.trim().is_empty() {
        return text.to_string();
    }
    let trimmed = text.trim();
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    let mut body = match style.capitalization {
        CapitalizationStyle::Default => trimmed.to_string(),
        CapitalizationStyle::Sentence => apply_sentence_caps(trimmed),
        CapitalizationStyle::Lowercase => apply_lowercase_starts(trimmed),
    };
    match style.punctuation {
        PunctuationStyle::Default => {}
        PunctuationStyle::DropTrailingPeriod if body.ends_with('.') && !body.ends_with("..") => {
            body.pop();
        }
        PunctuationStyle::DropTrailingPeriod => {}
        PunctuationStyle::Full => {
            if body
                .chars()
                .last()
                .is_some_and(|c| !matches!(c, '.' | '!' | '?' | ':' | ';'))
            {
                body.push('.');
            }
        }
    }
    format!("{lead}{body}{trail}")
}

fn apply_sentence_caps(text: &str) -> String {
    let (mut out, mut cap) = (String::with_capacity(text.len()), true);
    for ch in text.chars() {
        if cap && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            cap = false;
        } else {
            out.push(ch);
            cap = if matches!(ch, '.' | '!' | '?') {
                true
            } else if ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '[') {
                cap
            } else {
                false
            };
        }
    }
    out
}

fn apply_lowercase_starts(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut lower) = (String::with_capacity(text.len()), true);
    for i in 0..chars.len() {
        let ch = chars[i];
        if lower && ch.is_alphabetic() {
            let end = chars[i + 1..]
                .iter()
                .position(|c| !(c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}')))
                .map_or(chars.len(), |p| i + 1 + p);
            let word: String = chars[i..end].iter().collect();
            let keep = chars[i + 1..end].iter().any(|c| c.is_uppercase())
                || word == "I"
                || word.starts_with("I'")
                || word.starts_with("I\u{2019}");
            if ch.is_uppercase() && !keep {
                out.extend(ch.to_lowercase());
            } else {
                out.push(ch);
            }
            lower = false;
        } else {
            out.push(ch);
            lower = if matches!(ch, '.' | '!' | '?') {
                true
            } else if ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '[') {
                lower
            } else {
                false
            };
        }
    }
    out
}

#[cfg(test)]
mod per_app_tests {
    use super::*;
    use crate::settings::get_default_settings;

    fn app(name: &str, bundle: Option<&str>, stem: Option<&str>) -> ActiveAppInfo {
        ActiveAppInfo {
            app_name: name.into(),
            bundle_id: bundle.map(Into::into),
            process_stem: stem.map(Into::into),
        }
    }

    #[test]
    fn per_app_matching_kind_and_surface_style_preserve_words() {
        let slack = app("Slack", Some("com.tinyspeck.slackmacgap"), Some("Slack"));
        assert!(app_profile_matches("com.tinyspeck.slackmacgap", &slack));
        assert!(app_profile_matches("SLACK", &slack));
        assert!(!app_profile_matches(" ", &slack));
        let self_app = app(
            "Coco Voice",
            Some("com.cocoresearch.cocovoice"),
            Some("coco-voice"),
        );
        assert!(!app_profile_matches(
            "com.cocoresearch.cocovoice",
            &self_app
        ));

        let chat_style = tone_defaults(AppTone::Auto, Some(&slack));
        assert_eq!(
            apply_surface_style("Sounds good to me.", chat_style),
            "sounds good to me"
        );
        let mail = app("Mail", Some("com.apple.mail"), None);
        assert_eq!(
            apply_surface_style(
                "thanks for the note. see you soon",
                tone_defaults(AppTone::Auto, Some(&mail))
            ),
            "Thanks for the note. See you soon."
        );
        let parsed = parse_lsappinfo_info(
            "\"CFBundleIdentifier\"=\"com.microsoft.VSCode\"\n\"LSDisplayName\"=\"Code\"\n\"CFBundleExecutablePath\"=\"/Applications/Code.app/Contents/MacOS/Electron\"\n",
        )
        .unwrap();
        assert_eq!(classify_app_kind(&parsed), AppKind::Code);
        assert_eq!(
            apply_surface_style(
                "cargo test useState.",
                tone_defaults(AppTone::Code, Some(&parsed))
            ),
            "cargo test useState"
        );
    }

    #[test]
    fn per_app_overrides_and_blocked_bundle_noop_before_recording() {
        let mut s = get_default_settings();
        s.blocked_apps = vec!["com.1password.1password".into()];
        s.app_profiles = vec![
            AppProfile {
                id: "mail".into(),
                name: "Mail".into(),
                app_identifier: "com.apple.mail".into(),
                tone: AppTone::Auto,
                post_process_enabled: Some(true),
                corrections: vec![CorrectionPair {
                    from: "coco".into(),
                    to: "Coco Voice".into(),
                }],
                ..Default::default()
            },
            AppProfile {
                id: "bank".into(),
                name: "Bank".into(),
                app_identifier: "com.example.banking".into(),
                blocked: true,
                ..Default::default()
            },
        ];

        let onepass = app("1Password", Some("com.1password.1password"), None);
        assert_eq!(
            check_start_allowed(&s, Some(&onepass)),
            StartDecision::BlockedApp("com.1password.1password".into())
        );
        let bank = app("Banking", Some("com.example.banking"), None);
        assert!(check_start_allowed(&s, Some(&bank)).is_blocked());

        let mail = app("Mail", Some("com.apple.mail"), None);
        assert_eq!(check_start_allowed(&s, Some(&mail)), StartDecision::Allow);
        let resolved = resolve_for_app(&s, Some(&mail));
        assert_eq!(resolved.post_process_override, Some(true));
        assert_eq!(resolved.settings.corrections.len(), 1);
        assert_eq!(
            resolved.surface_style.capitalization,
            CapitalizationStyle::Sentence
        );

        let norm = normalize_app_profiles(
            vec![AppProfile {
                id: "v".into(),
                name: "".into(),
                app_identifier: "com.microsoft.VSCode".into(),
                prompt_id: Some("missing".into()),
                provider_id: Some("missing".into()),
                ..Default::default()
            }],
            &s,
        );
        assert!(norm[0].prompt_id.is_none() && norm[0].provider_id.is_none());
    }
}

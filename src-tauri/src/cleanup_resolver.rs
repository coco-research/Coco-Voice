//! Decides which provider runs transcript cleanup, and why it did not run.
//!
//! The selected provider is used when it is usable. When it is not, the
//! on-device model is the only fallback: cleanup never moves to a cloud
//! provider the user did not select.

use crate::settings::LOCAL_LLM_PROVIDER_ID;

/// Why cleanup did not run. `Display` is the text that goes in the log.
// Some variants are only produced on some platforms.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum SkipReason {
    EmptyTranscription,
    NoProviderSelected,
    NoModelConfigured,
    NoPromptSelected,
    PromptNotFound,
    PromptEmpty,
    NoApiKey,
    ProviderUnavailableInThisBuild,
    AppleIntelligenceUnavailable,
    LocalModelNotDownloaded,
    ProviderNotFound,
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::EmptyTranscription => write!(f, "transcription is empty"),
            SkipReason::NoProviderSelected => write!(f, "no provider is selected"),
            SkipReason::NoModelConfigured => write!(f, "no model configured for provider"),
            SkipReason::NoPromptSelected => write!(f, "no prompt is selected"),
            SkipReason::PromptNotFound => write!(f, "selected prompt was not found"),
            SkipReason::PromptEmpty => write!(f, "selected prompt is empty"),
            SkipReason::NoApiKey => write!(f, "no API key configured for provider"),
            SkipReason::ProviderUnavailableInThisBuild => {
                write!(f, "provider is not available on this platform")
            }
            SkipReason::AppleIntelligenceUnavailable => write!(
                f,
                "Apple Intelligence selected but not currently available on this device"
            ),
            SkipReason::LocalModelNotDownloaded => write!(f, "local LLM model is not downloaded"),
            SkipReason::ProviderNotFound => {
                write!(f, "selected provider was not found in settings")
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CleanupDecision {
    Run {
        provider_id: String,
        /// The selected provider and why it was not used, when the on-device
        /// model runs in its place.
        fallback_from: Option<(String, SkipReason)>,
    },
    Skip {
        reason: SkipReason,
    },
}

/// Checks that apply whatever the provider: something was said, and a
/// non-empty prompt is selected. A blank transcription is skipped because the
/// model would answer with an error message instead of cleaning anything.
/// Run this before [`resolve_cleanup`], so a missing prompt keeps cleanup off
/// even when the on-device model is ready.
pub fn check_global_preconditions<'a>(
    transcription: &str,
    selected_prompt_id: Option<&str>,
    get_prompt: impl Fn(&str) -> Option<&'a str>,
) -> Result<&'a str, SkipReason> {
    if transcription.trim().is_empty() {
        return Err(SkipReason::EmptyTranscription);
    }

    let prompt_id = selected_prompt_id.ok_or(SkipReason::NoPromptSelected)?;
    let prompt = get_prompt(prompt_id).ok_or(SkipReason::PromptNotFound)?;

    if prompt.trim().is_empty() {
        return Err(SkipReason::PromptEmpty);
    }

    Ok(prompt)
}

/// Picks the provider to run: the selected one, else the on-device model, else
/// a skip that names why the selected provider was unusable.
pub fn resolve_cleanup(
    selected_provider_id: &str,
    provider_usable: impl Fn(&str) -> Result<(), SkipReason>,
) -> CleanupDecision {
    if selected_provider_id.trim().is_empty() {
        return CleanupDecision::Skip {
            reason: SkipReason::NoProviderSelected,
        };
    }

    match provider_usable(selected_provider_id) {
        Ok(()) => CleanupDecision::Run {
            provider_id: selected_provider_id.to_string(),
            fallback_from: None,
        },
        Err(reason) => {
            if selected_provider_id != LOCAL_LLM_PROVIDER_ID {
                match provider_usable(LOCAL_LLM_PROVIDER_ID) {
                    Ok(()) => {
                        return CleanupDecision::Run {
                            provider_id: LOCAL_LLM_PROVIDER_ID.to_string(),
                            fallback_from: Some((selected_provider_id.to_string(), reason)),
                        };
                    }
                    Err(local_reason) => {
                        log::info!("Cleanup: on-device fallback unavailable ({})", local_reason);
                    }
                }
            }
            CleanupDecision::Skip { reason }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const LOCAL: &str = LOCAL_LLM_PROVIDER_ID;
    const APPLE: &str = crate::settings::APPLE_INTELLIGENCE_PROVIDER_ID;

    /// Resolves `selected` against fixed probe results (an unlisted id is not
    /// found) and returns the decision plus every id the resolver asked about.
    fn resolve(
        selected: &str,
        probes: &[(&str, Result<(), SkipReason>)],
    ) -> (CleanupDecision, Vec<String>) {
        let asked = RefCell::new(Vec::new());
        let decision = resolve_cleanup(selected, |id| {
            asked.borrow_mut().push(id.to_string());
            probes
                .iter()
                .find(|(probed, _)| *probed == id)
                .map_or(Err(SkipReason::ProviderNotFound), |(_, result)| {
                    result.clone()
                })
        });
        (decision, asked.into_inner())
    }

    fn run(provider_id: &str, fallback_from: Option<(&str, SkipReason)>) -> CleanupDecision {
        CleanupDecision::Run {
            provider_id: provider_id.to_string(),
            fallback_from: fallback_from.map(|(id, reason)| (id.to_string(), reason)),
        }
    }

    fn skip(reason: SkipReason) -> CleanupDecision {
        CleanupDecision::Skip { reason }
    }

    #[test]
    fn a_usable_selected_provider_runs_without_asking_about_the_fallback() {
        let (decision, asked) = resolve("openai", &[("openai", Ok(())), (LOCAL, Ok(()))]);
        assert_eq!(decision, run("openai", None));
        assert_eq!(asked, ["openai"]);
    }

    #[test]
    fn an_unusable_cloud_provider_falls_back_to_the_downloaded_on_device_model() {
        let (decision, _) = resolve(
            "openai",
            &[("openai", Err(SkipReason::NoApiKey)), (LOCAL, Ok(()))],
        );
        assert_eq!(decision, run(LOCAL, Some(("openai", SkipReason::NoApiKey))));

        let (decision, _) = resolve(
            APPLE,
            &[
                (APPLE, Err(SkipReason::AppleIntelligenceUnavailable)),
                (LOCAL, Ok(())),
            ],
        );
        assert_eq!(
            decision,
            run(
                LOCAL,
                Some((APPLE, SkipReason::AppleIntelligenceUnavailable))
            )
        );
    }

    #[test]
    fn without_a_usable_on_device_model_the_skip_names_the_selected_provider() {
        // Not downloaded yet.
        let (decision, _) = resolve(
            "openai",
            &[
                ("openai", Err(SkipReason::NoApiKey)),
                (LOCAL, Err(SkipReason::LocalModelNotDownloaded)),
            ],
        );
        assert_eq!(decision, skip(SkipReason::NoApiKey));

        // Not in this build (the macOS-only gate): the same skip, never a fallback.
        let (decision, _) = resolve(
            "openai",
            &[
                ("openai", Err(SkipReason::NoApiKey)),
                (LOCAL, Err(SkipReason::ProviderUnavailableInThisBuild)),
            ],
        );
        assert_eq!(decision, skip(SkipReason::NoApiKey));
    }

    #[test]
    fn cleanup_never_moves_to_a_cloud_provider_the_user_did_not_select() {
        let (decision, asked) = resolve(
            "openai",
            &[
                ("openai", Err(SkipReason::NoApiKey)),
                ("anthropic", Ok(())),
                (LOCAL, Err(SkipReason::LocalModelNotDownloaded)),
            ],
        );
        assert_eq!(decision, skip(SkipReason::NoApiKey));
        assert_eq!(asked, ["openai", LOCAL]);
    }

    #[test]
    fn a_selected_on_device_model_that_is_not_downloaded_skips_without_a_second_probe() {
        let (decision, asked) =
            resolve(LOCAL, &[(LOCAL, Err(SkipReason::LocalModelNotDownloaded))]);
        assert_eq!(decision, skip(SkipReason::LocalModelNotDownloaded));
        assert_eq!(asked, [LOCAL]);
    }

    #[test]
    fn no_selected_provider_skips_even_when_the_on_device_model_is_ready() {
        let (decision, asked) = resolve("   ", &[(LOCAL, Ok(()))]);
        assert_eq!(decision, skip(SkipReason::NoProviderSelected));
        assert!(asked.is_empty());
    }

    #[test]
    fn preconditions_keep_cleanup_off_whatever_the_provider() {
        let prompt = |id: &str| match id {
            "valid" => Some("prompt text"),
            "blank" => Some("   "),
            _ => None,
        };

        for blank in ["", "   ", "\t\n  \r\n"] {
            assert_eq!(
                check_global_preconditions(blank, Some("valid"), prompt),
                Err(SkipReason::EmptyTranscription)
            );
        }
        assert_eq!(
            check_global_preconditions("text", None, prompt),
            Err(SkipReason::NoPromptSelected)
        );
        assert_eq!(
            check_global_preconditions("text", Some("missing"), prompt),
            Err(SkipReason::PromptNotFound)
        );
        assert_eq!(
            check_global_preconditions("text", Some("blank"), prompt),
            Err(SkipReason::PromptEmpty)
        );
        assert_eq!(
            check_global_preconditions("  text  ", Some("valid"), prompt),
            Ok("prompt text")
        );
    }
}

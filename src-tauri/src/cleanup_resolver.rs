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

pub enum CleanupDecision {
    Run {
        provider_id: String,
        fallback_from: Option<(String, SkipReason)>,
    },
    Skip {
        reason: SkipReason,
    },
}

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
            if selected_provider_id != crate::settings::LOCAL_LLM_PROVIDER_ID {
                match provider_usable(crate::settings::LOCAL_LLM_PROVIDER_ID) {
                    Ok(()) => {
                        return CleanupDecision::Run {
                            provider_id: crate::settings::LOCAL_LLM_PROVIDER_ID.to_string(),
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

    #[test]
    fn test_resolve_cleanup() {
        // selected cloud usable -> run it
        let provider_usable_1 = |_id: &str| -> Result<(), SkipReason> { Ok(()) };
        let decision = resolve_cleanup("openai", &provider_usable_1);
        assert!(matches!(
            decision,
            CleanupDecision::Run {
                ref provider_id,
                fallback_from: None
            } if provider_id == "openai"
        ));

        // selected OpenAI without API key + local downloaded -> run local
        let provider_usable_2 = |id: &str| -> Result<(), SkipReason> {
            if id == crate::settings::LOCAL_LLM_PROVIDER_ID {
                Ok(())
            } else {
                Err(SkipReason::NoApiKey)
            }
        };
        let decision = resolve_cleanup("openai", &provider_usable_2);
        assert!(matches!(
            decision,
            CleanupDecision::Run {
                ref provider_id,
                fallback_from: Some((ref fallback_id, SkipReason::NoApiKey))
            } if provider_id == crate::settings::LOCAL_LLM_PROVIDER_ID && fallback_id == "openai"
        ));

        // selected OpenAI without API key + local not downloaded -> skip NoApiKey
        let provider_usable_3 = |id: &str| -> Result<(), SkipReason> {
            if id == crate::settings::LOCAL_LLM_PROVIDER_ID {
                Err(SkipReason::LocalModelNotDownloaded)
            } else {
                Err(SkipReason::NoApiKey)
            }
        };
        let decision = resolve_cleanup("openai", &provider_usable_3);
        assert!(matches!(
            decision,
            CleanupDecision::Skip {
                reason: SkipReason::NoApiKey
            }
        ));

        // selected OpenAI without API key + local unavailable in this build -> skip NoApiKey (never falls back when closure denies local for any reason)
        let provider_usable_4 = |id: &str| -> Result<(), SkipReason> {
            if id == crate::settings::LOCAL_LLM_PROVIDER_ID {
                Err(SkipReason::ProviderUnavailableInThisBuild)
            } else {
                Err(SkipReason::NoApiKey)
            }
        };
        let decision = resolve_cleanup("openai", &provider_usable_4);
        assert!(matches!(
            decision,
            CleanupDecision::Skip {
                reason: SkipReason::NoApiKey
            }
        ));

        // Apple Intelligence unavailable + local downloaded -> run local
        let provider_usable_apple = |id: &str| -> Result<(), SkipReason> {
            if id == crate::settings::LOCAL_LLM_PROVIDER_ID {
                Ok(())
            } else {
                Err(SkipReason::AppleIntelligenceUnavailable)
            }
        };
        let decision = resolve_cleanup(
            crate::settings::APPLE_INTELLIGENCE_PROVIDER_ID,
            &provider_usable_apple,
        );
        assert!(matches!(
            decision,
            CleanupDecision::Run {
                ref provider_id,
                fallback_from: Some((ref fallback_id, SkipReason::AppleIntelligenceUnavailable))
            } if provider_id == crate::settings::LOCAL_LLM_PROVIDER_ID && fallback_id == crate::settings::APPLE_INTELLIGENCE_PROVIDER_ID
        ));

        // empty provider id -> Skip(NoProviderSelected)
        let decision = resolve_cleanup("   ", &provider_usable_3);
        assert!(matches!(
            decision,
            CleanupDecision::Skip {
                reason: SkipReason::NoProviderSelected
            }
        ));

        // selected local not downloaded -> skip LocalModelNotDownloaded
        let provider_usable_local_only =
            |_id: &str| -> Result<(), SkipReason> { Err(SkipReason::LocalModelNotDownloaded) };
        let decision = resolve_cleanup(
            crate::settings::LOCAL_LLM_PROVIDER_ID,
            &provider_usable_local_only,
        );
        assert!(matches!(
            decision,
            CleanupDecision::Skip {
                reason: SkipReason::LocalModelNotDownloaded
            }
        ));
    }

    #[test]
    fn test_check_global_preconditions() {
        let get_prompt = |id: &str| -> Option<&'static str> {
            match id {
                "valid" => Some("prompt text"),
                "empty" => Some("   "),
                _ => None,
            }
        };

        // blank transcription + local downloaded -> Skip(EmptyTranscription)
        assert_eq!(
            check_global_preconditions("   ", Some("valid"), &get_prompt),
            Err(SkipReason::EmptyTranscription)
        );

        // no prompt selected + local downloaded -> Skip(NoPromptSelected)
        assert_eq!(
            check_global_preconditions("text", None, &get_prompt),
            Err(SkipReason::NoPromptSelected)
        );

        assert_eq!(
            check_global_preconditions("text", Some("missing"), &get_prompt),
            Err(SkipReason::PromptNotFound)
        );

        assert_eq!(
            check_global_preconditions("text", Some("empty"), &get_prompt),
            Err(SkipReason::PromptEmpty)
        );

        assert_eq!(
            check_global_preconditions("text", Some("valid"), &get_prompt),
            Ok("prompt text")
        );
    }
}

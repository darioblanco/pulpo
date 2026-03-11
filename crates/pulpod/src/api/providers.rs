use axum::Json;
use pulpo_common::api::{ProviderCapabilitiesResponse, ProviderInfoResponse, ProvidersResponse};
use pulpo_common::session::Provider;

use crate::guard::provider_capabilities;
use crate::session::manager::{is_provider_available, provider_binary};

/// All known providers in the order they should be listed.
const ALL_PROVIDERS: [Provider; 5] = [
    Provider::Claude,
    Provider::Codex,
    Provider::Gemini,
    Provider::OpenCode,
    Provider::Shell,
];

pub async fn list() -> Json<ProvidersResponse> {
    let providers = ALL_PROVIDERS
        .iter()
        .map(|&p| {
            let caps = provider_capabilities(p);
            ProviderInfoResponse {
                provider: p,
                binary: provider_binary(p),
                available: is_provider_available(p),
                capabilities: ProviderCapabilitiesResponse {
                    model: caps.model,
                    system_prompt: caps.system_prompt,
                    allowed_tools: caps.allowed_tools,
                    max_turns: caps.max_turns,
                    max_budget_usd: caps.max_budget_usd,
                    output_format: caps.output_format,
                    worktree: caps.worktree,
                    unrestricted: caps.unrestricted,
                    resume: caps.resume,
                },
            }
        })
        .collect();
    Json(ProvidersResponse { providers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_providers_returns_all() {
        let Json(resp) = list().await;
        assert_eq!(resp.providers.len(), 5);
        let names: Vec<_> = resp.providers.iter().map(|p| p.provider).collect();
        assert!(names.contains(&Provider::Claude));
        assert!(names.contains(&Provider::Codex));
        assert!(names.contains(&Provider::Gemini));
        assert!(names.contains(&Provider::OpenCode));
        assert!(names.contains(&Provider::Shell));
    }

    #[tokio::test]
    async fn test_shell_always_available() {
        let Json(resp) = list().await;
        let shell = resp
            .providers
            .iter()
            .find(|p| p.provider == Provider::Shell)
            .unwrap();
        assert!(shell.available);
        assert_eq!(shell.binary, "bash");
    }

    #[tokio::test]
    async fn test_shell_capabilities_all_false() {
        let Json(resp) = list().await;
        let shell = resp
            .providers
            .iter()
            .find(|p| p.provider == Provider::Shell)
            .unwrap();
        let caps = &shell.capabilities;
        assert!(!caps.model);
        assert!(!caps.system_prompt);
        assert!(!caps.allowed_tools);
        assert!(!caps.max_turns);
        assert!(!caps.max_budget_usd);
        assert!(!caps.output_format);
        assert!(!caps.worktree);
        assert!(!caps.unrestricted);
        assert!(!caps.resume);
    }

    #[tokio::test]
    async fn test_claude_capabilities() {
        let Json(resp) = list().await;
        let claude = resp
            .providers
            .iter()
            .find(|p| p.provider == Provider::Claude)
            .unwrap();
        let caps = &claude.capabilities;
        assert!(caps.model);
        assert!(caps.system_prompt);
        assert!(caps.allowed_tools);
        assert!(caps.max_turns);
        assert!(caps.max_budget_usd);
        assert!(caps.output_format);
        assert!(caps.worktree);
        assert!(caps.unrestricted);
        assert!(caps.resume);
    }

    #[tokio::test]
    async fn test_provider_info_has_binary_name() {
        let Json(resp) = list().await;
        for p in &resp.providers {
            assert!(
                !p.binary.is_empty(),
                "binary should not be empty for {:?}",
                p.provider
            );
        }
    }

    #[test]
    fn test_all_providers_constant() {
        assert_eq!(ALL_PROVIDERS.len(), 5);
    }
}

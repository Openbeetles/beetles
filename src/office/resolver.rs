use crate::office::{
    OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability, OfficeCapabilityBinding,
    OfficeSelectionPolicy,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeResolveRequest {
    pub capability: OfficeCapability,
    pub preferred_account_key: Option<String>,
    pub preferred_provider_kind: Option<String>,
    pub preferred_identity_class: Option<OfficeAccountIdentityClass>,
    pub historical_account_key: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeResolveSelection {
    pub account_key: String,
    pub selection_reason: OfficeResolveSelectionReason,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeResolveCandidate {
    pub account_key: String,
    pub provider_kind: String,
    pub account_label: String,
    pub identity_class: OfficeAccountIdentityClass,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeResolveSelectionReason {
    ExplicitAccountKey,
    CapabilityDefault,
    HistoricalSuccessfulActivity,
    GlobalDefault,
    PreferredIdentityClass,
    SoleCandidate,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeResolveAmbiguityReason {
    MultipleMatchingAccounts,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeResolveAmbiguity {
    pub reason: OfficeResolveAmbiguityReason,
    pub candidate_accounts: Vec<OfficeResolveCandidate>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeResolveMissingReason {
    NoMatchingAccounts,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeResolveMissing {
    pub reason: OfficeResolveMissingReason,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OfficeResolveResult {
    Selected(OfficeResolveSelection),
    Ambiguous(OfficeResolveAmbiguity),
    Missing(OfficeResolveMissing),
}

pub struct OfficeResolver;

impl OfficeResolver {
    pub fn resolve(
        registry: &OfficeAccountRegistry,
        binding: &OfficeCapabilityBinding,
        policy: &OfficeSelectionPolicy,
        request: &OfficeResolveRequest,
    ) -> OfficeResolveResult {
        let mut candidates = registry.accounts_for_capability(request.capability);
        if let Some(provider_kind) = request
            .preferred_provider_kind
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            candidates.retain(|account| account.provider_kind == provider_kind);
        }

        if let Some(account_key) = request.preferred_account_key.as_deref() {
            if account_in_candidates(&candidates, account_key) {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: account_key.to_string(),
                    selection_reason: OfficeResolveSelectionReason::ExplicitAccountKey,
                });
            }
        }

        if let Some(identity_class) = request.preferred_identity_class {
            candidates.retain(|account| account.identity_class == identity_class);
            match candidates.len() {
                0 => {
                    return OfficeResolveResult::Missing(OfficeResolveMissing {
                        reason: OfficeResolveMissingReason::NoMatchingAccounts,
                    });
                }
                1 => {
                    return OfficeResolveResult::Selected(OfficeResolveSelection {
                        account_key: candidates[0].account_key.clone(),
                        selection_reason: OfficeResolveSelectionReason::PreferredIdentityClass,
                    });
                }
                _ => {}
            }
        }

        if let Some(account_key) = binding.default_account_for(request.capability) {
            if account_in_candidates(&candidates, account_key) {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: account_key.to_string(),
                    selection_reason: OfficeResolveSelectionReason::CapabilityDefault,
                });
            }
        }

        if let Some(account_key) = request
            .historical_account_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if account_in_candidates(&candidates, account_key) {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: account_key.to_string(),
                    selection_reason: OfficeResolveSelectionReason::HistoricalSuccessfulActivity,
                });
            }
        }

        if !policy.global_default_account_key.is_empty()
            && account_in_candidates(&candidates, &policy.global_default_account_key)
        {
            return OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: policy.global_default_account_key.clone(),
                selection_reason: OfficeResolveSelectionReason::GlobalDefault,
            });
        }

        if let Some(identity_class) = policy.preferred_identity_class {
            let matching = candidates
                .iter()
                .copied()
                .filter(|account| account.identity_class == identity_class)
                .collect::<Vec<_>>();
            if matching.len() == 1 {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: matching[0].account_key.clone(),
                    selection_reason: OfficeResolveSelectionReason::PreferredIdentityClass,
                });
            }
            if !matching.is_empty() {
                candidates = matching;
            }
        }
        match candidates.len() {
            0 => OfficeResolveResult::Missing(OfficeResolveMissing {
                reason: OfficeResolveMissingReason::NoMatchingAccounts,
            }),
            1 => OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: candidates[0].account_key.clone(),
                selection_reason: OfficeResolveSelectionReason::SoleCandidate,
            }),
            _ => OfficeResolveResult::Ambiguous(OfficeResolveAmbiguity {
                reason: OfficeResolveAmbiguityReason::MultipleMatchingAccounts,
                candidate_accounts: candidates
                    .into_iter()
                    .map(OfficeResolveCandidate::from_account)
                    .collect(),
            }),
        }
    }
}

impl OfficeResolveCandidate {
    pub fn from_account(account: &crate::office::OfficeAccount) -> Self {
        Self {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            account_label: account.account_label.clone(),
            identity_class: account.identity_class,
        }
    }
}

fn account_in_candidates(candidates: &[&crate::office::OfficeAccount], account_key: &str) -> bool {
    candidates
        .iter()
        .any(|account| account.account_key == account_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{OfficeAccount, OfficeAccountIdentityClass, OfficeCapability};

    fn account(
        account_key: &str,
        provider_kind: &str,
        identity_class: OfficeAccountIdentityClass,
        enabled_capabilities: Vec<OfficeCapability>,
    ) -> OfficeAccount {
        OfficeAccount {
            account_key: account_key.to_string(),
            provider_kind: provider_kind.to_string(),
            external_account_id: account_key.to_string(),
            account_label: account_key.to_string(),
            identity_class,
            enabled_capabilities,
        }
    }

    #[test]
    fn resolve_prefers_explicit_account_key() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(account(
            "mail-work",
            "imap_smtp",
            OfficeAccountIdentityClass::Work,
            vec![OfficeCapability::Mail],
        ));
        registry.insert(account(
            "mail-personal",
            "imap_smtp",
            OfficeAccountIdentityClass::Personal,
            vec![OfficeCapability::Mail],
        ));
        let result = OfficeResolver::resolve(
            &registry,
            &OfficeCapabilityBinding::default(),
            &OfficeSelectionPolicy::default(),
            &OfficeResolveRequest {
                capability: OfficeCapability::Mail,
                preferred_account_key: Some("mail-personal".to_string()),
                preferred_provider_kind: None,
                preferred_identity_class: None,
                historical_account_key: None,
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "mail-personal".to_string(),
                selection_reason: OfficeResolveSelectionReason::ExplicitAccountKey,
            })
        );
    }

    #[test]
    fn resolve_uses_capability_default_before_global_default() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(account(
            "calendar-work",
            "caldav",
            OfficeAccountIdentityClass::Work,
            vec![OfficeCapability::Calendar],
        ));
        registry.insert(account(
            "calendar-personal",
            "caldav",
            OfficeAccountIdentityClass::Personal,
            vec![OfficeCapability::Calendar],
        ));
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Calendar, "calendar-work".to_string());
        let policy = OfficeSelectionPolicy {
            global_default_account_key: "calendar-personal".to_string(),
            ask_when_ambiguous: true,
            preferred_identity_class: None,
        };
        let result = OfficeResolver::resolve(
            &registry,
            &binding,
            &policy,
            &OfficeResolveRequest {
                capability: OfficeCapability::Calendar,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: None,
                historical_account_key: None,
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "calendar-work".to_string(),
                selection_reason: OfficeResolveSelectionReason::CapabilityDefault,
            })
        );
    }

    #[test]
    fn resolve_prefers_identity_class_before_ambiguity() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(account(
            "mail-work",
            "imap_smtp",
            OfficeAccountIdentityClass::Work,
            vec![OfficeCapability::Mail],
        ));
        registry.insert(account(
            "mail-personal",
            "imap_smtp",
            OfficeAccountIdentityClass::Personal,
            vec![OfficeCapability::Mail],
        ));
        let result = OfficeResolver::resolve(
            &registry,
            &OfficeCapabilityBinding::default(),
            &OfficeSelectionPolicy::default(),
            &OfficeResolveRequest {
                capability: OfficeCapability::Mail,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: Some(OfficeAccountIdentityClass::Work),
                historical_account_key: None,
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "mail-work".to_string(),
                selection_reason: OfficeResolveSelectionReason::PreferredIdentityClass,
            })
        );
    }

    #[test]
    fn resolve_explicit_identity_class_overrides_default_binding() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(account(
            "mail-work",
            "imap_smtp",
            OfficeAccountIdentityClass::Work,
            vec![OfficeCapability::Mail],
        ));
        registry.insert(account(
            "mail-personal",
            "imap_smtp",
            OfficeAccountIdentityClass::Personal,
            vec![OfficeCapability::Mail],
        ));
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Mail, "mail-personal".to_string());
        let result = OfficeResolver::resolve(
            &registry,
            &binding,
            &OfficeSelectionPolicy::default(),
            &OfficeResolveRequest {
                capability: OfficeCapability::Mail,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: Some(OfficeAccountIdentityClass::Work),
                historical_account_key: None,
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "mail-work".to_string(),
                selection_reason: OfficeResolveSelectionReason::PreferredIdentityClass,
            })
        );
    }

    #[test]
    fn resolve_reports_ambiguity_when_multiple_accounts_match_without_policy() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(account(
            "docs-shared-a",
            "webdav",
            OfficeAccountIdentityClass::Shared,
            vec![OfficeCapability::Documents],
        ));
        registry.insert(account(
            "docs-shared-b",
            "webdav",
            OfficeAccountIdentityClass::Shared,
            vec![OfficeCapability::Documents],
        ));
        let result = OfficeResolver::resolve(
            &registry,
            &OfficeCapabilityBinding::default(),
            &OfficeSelectionPolicy::default(),
            &OfficeResolveRequest {
                capability: OfficeCapability::Documents,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: None,
                historical_account_key: None,
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Ambiguous(OfficeResolveAmbiguity {
                reason: OfficeResolveAmbiguityReason::MultipleMatchingAccounts,
                candidate_accounts: vec![
                    OfficeResolveCandidate {
                        account_key: "docs-shared-a".to_string(),
                        provider_kind: "webdav".to_string(),
                        account_label: "docs-shared-a".to_string(),
                        identity_class: OfficeAccountIdentityClass::Shared,
                    },
                    OfficeResolveCandidate {
                        account_key: "docs-shared-b".to_string(),
                        provider_kind: "webdav".to_string(),
                        account_label: "docs-shared-b".to_string(),
                        identity_class: OfficeAccountIdentityClass::Shared,
                    },
                ],
            })
        );
    }

    #[test]
    fn resolve_prefers_historical_successful_activity_before_global_default() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(account(
            "docs-work",
            "webdav",
            OfficeAccountIdentityClass::Work,
            vec![OfficeCapability::Documents],
        ));
        registry.insert(account(
            "docs-personal",
            "webdav",
            OfficeAccountIdentityClass::Personal,
            vec![OfficeCapability::Documents],
        ));
        let policy = OfficeSelectionPolicy {
            global_default_account_key: "docs-personal".to_string(),
            ask_when_ambiguous: true,
            preferred_identity_class: None,
        };
        let result = OfficeResolver::resolve(
            &registry,
            &OfficeCapabilityBinding::default(),
            &policy,
            &OfficeResolveRequest {
                capability: OfficeCapability::Documents,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: None,
                historical_account_key: Some("docs-work".to_string()),
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "docs-work".to_string(),
                selection_reason: OfficeResolveSelectionReason::HistoricalSuccessfulActivity,
            })
        );
    }
}

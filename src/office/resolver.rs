use crate::office::{
    OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability, OfficeCapabilityBinding,
    OfficeSelectionPolicy,
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeResolveRequest {
    pub capability: OfficeCapability,
    pub preferred_account_key: Option<String>,
    pub preferred_identity_class: Option<OfficeAccountIdentityClass>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeResolveSelection {
    pub account_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeResolveResult {
    Selected(OfficeResolveSelection),
    Ambiguous,
    Missing,
}

pub struct OfficeResolver;

impl OfficeResolver {
    pub fn resolve(
        registry: &OfficeAccountRegistry,
        binding: &OfficeCapabilityBinding,
        policy: &OfficeSelectionPolicy,
        request: &OfficeResolveRequest,
    ) -> OfficeResolveResult {
        if let Some(account_key) = request.preferred_account_key.as_deref() {
            if account_supports_capability(registry, account_key, request.capability) {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: account_key.to_string(),
                });
            }
        }

        if let Some(account_key) = binding.default_account_for(request.capability) {
            if account_supports_capability(registry, account_key, request.capability) {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: account_key.to_string(),
                });
            }
        }

        if !policy.global_default_account_key.is_empty() {
            if account_supports_capability(
                registry,
                &policy.global_default_account_key,
                request.capability,
            ) {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: policy.global_default_account_key.clone(),
                });
            }
        }

        let mut candidates = registry.accounts_for_capability(request.capability);
        let preferred_identity_class = request
            .preferred_identity_class
            .or(policy.preferred_identity_class);
        if let Some(identity_class) = preferred_identity_class {
            let matching = candidates
                .iter()
                .copied()
                .filter(|account| account.identity_class == identity_class)
                .collect::<Vec<_>>();
            if matching.len() == 1 {
                return OfficeResolveResult::Selected(OfficeResolveSelection {
                    account_key: matching[0].account_key.clone(),
                });
            }
            if !matching.is_empty() {
                candidates = matching;
            }
        }
        match candidates.len() {
            0 => OfficeResolveResult::Missing,
            1 => OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: candidates[0].account_key.clone(),
            }),
            _ => OfficeResolveResult::Ambiguous,
        }
    }
}

fn account_supports_capability(
    registry: &OfficeAccountRegistry,
    account_key: &str,
    capability: crate::office::OfficeCapability,
) -> bool {
    registry
        .get(account_key)
        .is_some_and(|account| account.enabled_capabilities.contains(&capability))
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
                preferred_identity_class: None,
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "mail-personal".to_string()
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
                preferred_identity_class: None,
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "calendar-work".to_string()
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
                preferred_identity_class: Some(OfficeAccountIdentityClass::Work),
            },
        );
        assert_eq!(
            result,
            OfficeResolveResult::Selected(OfficeResolveSelection {
                account_key: "mail-work".to_string()
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
                preferred_identity_class: None,
            },
        );
        assert_eq!(result, OfficeResolveResult::Ambiguous);
    }
}

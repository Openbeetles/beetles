macro_rules! define_provider_registry {
    ($registry:ident, $provider_trait:path) => {
        #[derive(Clone, Default)]
        pub struct $registry {
            providers: std::collections::HashMap<&'static str, std::sync::Arc<dyn $provider_trait>>,
        }

        impl $registry {
            pub fn new() -> Self {
                Self {
                    providers: std::collections::HashMap::new(),
                }
            }

            pub fn register(&mut self, provider: std::sync::Arc<dyn $provider_trait>) {
                self.providers.insert(provider.provider_name(), provider);
            }

            pub fn get(&self, provider: &str) -> Option<std::sync::Arc<dyn $provider_trait>> {
                self.providers.get(provider).cloned()
            }

            pub fn names(&self) -> Vec<&'static str> {
                let mut names = self.providers.keys().copied().collect::<Vec<_>>();
                names.sort_unstable();
                names
            }
        }
    };
}

macro_rules! define_office_backed_credential_store {
    (
        store = $store:ident,
        trait = $store_trait:path,
        credential = $credential:path,
        status = $status:path,
        capability = $capability:expr,
        from_office = $from_office:expr,
        status_from_office = $status_from_office:expr
    ) => {
        #[derive(Clone)]
        pub struct $store {
            core: crate::office::OfficeAuthorityBackedCredentialStoreCore,
        }

        impl $store {
            pub fn new(office: crate::office::OfficeService) -> Self {
                Self {
                    core: crate::office::OfficeAuthorityBackedCredentialStoreCore::new(office),
                }
            }

            pub fn with_authority(
                authority: std::sync::Arc<dyn crate::office::OfficeAuthoritySource + Send + Sync>,
            ) -> Self {
                Self {
                    core: crate::office::OfficeAuthorityBackedCredentialStoreCore::with_authority(
                        authority,
                    ),
                }
            }
        }

        impl $store_trait for $store {
            fn get(&self, account_key: &str) -> crate::error::Result<Option<$credential>> {
                self.core
                    .get_for_capability($capability, account_key, $from_office)
            }

            fn find_account_keys_by_provider(
                &self,
                provider: &str,
            ) -> crate::error::Result<Vec<String>> {
                self.core
                    .find_account_keys_by_provider($capability, provider)
            }

            fn list_statuses(&self) -> crate::error::Result<Vec<$status>> {
                self.core
                    .list_statuses_for_capability($capability, |account, credential| {
                        ($status_from_office)(account, credential)
                    })
            }
        }
    };
    (
        store = $store:ident,
        trait = $store_trait:path,
        credential = $credential:path,
        status = $status:path,
        capability = $capability:expr,
        from_office = $from_office:expr,
        status_from_office = $status_from_office:expr,
        set = ($set_self:ident, $set_credential:ident) $set:block,
        clear = ($clear_self:ident, $clear_account_key:ident) $clear:block $(,)?
    ) => {
        #[derive(Clone)]
        pub struct $store {
            core: crate::office::OfficeAuthorityBackedCredentialStoreCore,
        }

        impl $store {
            pub fn new(office: crate::office::OfficeService) -> Self {
                Self {
                    core: crate::office::OfficeAuthorityBackedCredentialStoreCore::new(office),
                }
            }

            pub fn with_authority(
                authority: std::sync::Arc<dyn crate::office::OfficeAuthoritySource + Send + Sync>,
            ) -> Self {
                Self {
                    core: crate::office::OfficeAuthorityBackedCredentialStoreCore::with_authority(
                        authority,
                    ),
                }
            }
        }

        impl $store_trait for $store {
            fn get(&self, account_key: &str) -> crate::error::Result<Option<$credential>> {
                self.core
                    .get_for_capability($capability, account_key, $from_office)
            }

            fn find_account_keys_by_provider(
                &self,
                provider: &str,
            ) -> crate::error::Result<Vec<String>> {
                self.core
                    .find_account_keys_by_provider($capability, provider)
            }

            fn set(&self, credential: &$credential) -> crate::error::Result<()> {
                let $set_self = self;
                let $set_credential = credential;
                $set
            }

            fn clear(&self, account_key: &str) -> crate::error::Result<()> {
                let $clear_self = self;
                let $clear_account_key = account_key;
                $clear
            }

            fn list_statuses(&self) -> crate::error::Result<Vec<$status>> {
                self.core
                    .list_statuses_for_capability($capability, |account, credential| {
                        ($status_from_office)(account, credential)
                    })
            }
        }
    };
}

pub(crate) use define_office_backed_credential_store;
pub(crate) use define_provider_registry;

use crate::office::OfficeProbeAdapter;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct OfficeIntegrationTopology {
    mail_providers: crate::mail::MailProviderRegistry,
    documents_providers: crate::documents::DocumentsProviderRegistry,
    calendar_providers: crate::calendar::CalendarProviderRegistry,
    contacts_directory_providers: crate::contacts_directory::ContactsDirectoryProviderRegistry,
    probe_adapters: Vec<Arc<dyn OfficeProbeAdapter + Send + Sync>>,
}

impl OfficeIntegrationTopology {
    fn new() -> Self {
        Self {
            mail_providers: crate::mail::MailProviderRegistry::new(),
            documents_providers: crate::documents::DocumentsProviderRegistry::new(),
            calendar_providers: crate::calendar::CalendarProviderRegistry::new(),
            contacts_directory_providers:
                crate::contacts_directory::ContactsDirectoryProviderRegistry::new(),
            probe_adapters: Vec::new(),
        }
    }

    pub(crate) fn mail_providers(&self) -> crate::mail::MailProviderRegistry {
        self.mail_providers.clone()
    }

    pub(crate) fn documents_providers(&self) -> crate::documents::DocumentsProviderRegistry {
        self.documents_providers.clone()
    }

    pub(crate) fn calendar_providers(&self) -> crate::calendar::CalendarProviderRegistry {
        self.calendar_providers.clone()
    }

    pub(crate) fn contacts_directory_providers(
        &self,
    ) -> crate::contacts_directory::ContactsDirectoryProviderRegistry {
        self.contacts_directory_providers.clone()
    }

    pub(crate) fn probe_adapters(&self) -> Vec<Arc<dyn OfficeProbeAdapter + Send + Sync>> {
        self.probe_adapters.clone()
    }

    fn register_mail(
        &mut self,
        provider: Arc<dyn crate::mail::MailProvider>,
        probe_adapter: Arc<dyn OfficeProbeAdapter + Send + Sync>,
    ) {
        self.mail_providers.register(provider);
        self.probe_adapters.push(probe_adapter);
    }

    fn register_documents(
        &mut self,
        provider: Arc<dyn crate::documents::DocumentsProvider>,
        probe_adapter: Arc<dyn OfficeProbeAdapter + Send + Sync>,
    ) {
        self.documents_providers.register(provider);
        self.probe_adapters.push(probe_adapter);
    }

    fn register_calendar(
        &mut self,
        provider: Arc<dyn crate::calendar::CalendarProvider>,
        probe_adapter: Arc<dyn OfficeProbeAdapter + Send + Sync>,
    ) {
        self.calendar_providers.register(provider);
        self.probe_adapters.push(probe_adapter);
    }

    fn register_contacts_directory(
        &mut self,
        provider: Arc<dyn crate::contacts_directory::ContactsDirectoryProvider>,
        probe_adapter: Arc<dyn OfficeProbeAdapter + Send + Sync>,
    ) {
        self.contacts_directory_providers.register(provider);
        self.probe_adapters.push(probe_adapter);
    }
}

pub(crate) fn build_default_office_integration_topology() -> OfficeIntegrationTopology {
    let mut topology = OfficeIntegrationTopology::new();

    topology.register_mail(
        Arc::new(crate::mail::providers::imap_smtp::ImapSmtpProvider),
        Arc::new(crate::mail::providers::imap_smtp::ImapSmtpOfficeProbeAdapter),
    );
    topology.register_mail(
        Arc::new(crate::mail::providers::feishu::FeishuMailProvider),
        Arc::new(crate::mail::providers::feishu::FeishuMailOfficeProbeAdapter),
    );
    topology.register_mail(
        Arc::new(crate::mail::providers::wecom::WecomMailProvider),
        Arc::new(crate::mail::providers::wecom::WecomMailOfficeProbeAdapter),
    );
    topology.register_mail(
        Arc::new(crate::mail::providers::microsoft365::Microsoft365MailProvider),
        Arc::new(crate::mail::providers::microsoft365::Microsoft365MailOfficeProbeAdapter),
    );
    topology.register_mail(
        Arc::new(crate::mail::providers::google::GoogleMailProvider),
        Arc::new(crate::mail::providers::google::GoogleMailOfficeProbeAdapter),
    );

    topology.register_documents(
        Arc::new(crate::documents::providers::webdav::WebDavProvider),
        Arc::new(crate::documents::providers::webdav::WebDavOfficeProbeAdapter),
    );
    topology.register_documents(
        Arc::new(crate::documents::providers::feishu::FeishuDocumentsProvider),
        Arc::new(crate::documents::providers::feishu::FeishuDocumentsOfficeProbeAdapter),
    );
    topology.register_documents(
        Arc::new(crate::documents::providers::wecom::WecomDocumentsProvider),
        Arc::new(crate::documents::providers::wecom::WecomDocumentsOfficeProbeAdapter),
    );
    topology.register_documents(
        Arc::new(crate::documents::providers::microsoft365::Microsoft365DocumentsProvider),
        Arc::new(
            crate::documents::providers::microsoft365::Microsoft365DocumentsOfficeProbeAdapter,
        ),
    );
    topology.register_documents(
        Arc::new(crate::documents::providers::google::GoogleDocumentsProvider),
        Arc::new(crate::documents::providers::google::GoogleDocumentsOfficeProbeAdapter),
    );

    topology.register_calendar(
        Arc::new(crate::calendar::providers::caldav::CalDavProvider),
        Arc::new(crate::calendar::providers::caldav::CalDavOfficeProbeAdapter),
    );
    topology.register_calendar(
        Arc::new(crate::calendar::providers::feishu::FeishuCalendarProvider),
        Arc::new(crate::calendar::providers::feishu::FeishuCalendarOfficeProbeAdapter),
    );
    topology.register_calendar(
        Arc::new(crate::calendar::providers::wecom::WecomCalendarProvider),
        Arc::new(crate::calendar::providers::wecom::WecomCalendarOfficeProbeAdapter),
    );
    topology.register_calendar(
        Arc::new(crate::calendar::providers::microsoft365::Microsoft365CalendarProvider),
        Arc::new(crate::calendar::providers::microsoft365::Microsoft365CalendarOfficeProbeAdapter),
    );
    topology.register_calendar(
        Arc::new(crate::calendar::providers::google::GoogleCalendarProvider),
        Arc::new(crate::calendar::providers::google::GoogleCalendarOfficeProbeAdapter),
    );

    topology.register_contacts_directory(
        Arc::new(crate::contacts_directory::providers::feishu::FeishuContactsDirectoryProvider),
        Arc::new(
            crate::contacts_directory::providers::feishu::FeishuContactsDirectoryOfficeProbeAdapter,
        ),
    );
    topology.register_contacts_directory(
        Arc::new(crate::contacts_directory::providers::wecom::WecomContactsDirectoryProvider),
        Arc::new(
            crate::contacts_directory::providers::wecom::WecomContactsDirectoryOfficeProbeAdapter,
        ),
    );
    topology.register_contacts_directory(
        Arc::new(
            crate::contacts_directory::providers::microsoft365::Microsoft365ContactsDirectoryProvider,
        ),
        Arc::new(
            crate::contacts_directory::providers::microsoft365::Microsoft365ContactsDirectoryOfficeProbeAdapter,
        ),
    );
    topology.register_contacts_directory(
        Arc::new(crate::contacts_directory::providers::google::GoogleContactsDirectoryProvider),
        Arc::new(
            crate::contacts_directory::providers::google::GoogleContactsDirectoryOfficeProbeAdapter,
        ),
    );

    topology
}

#[cfg(test)]
mod tests {
    use super::build_default_office_integration_topology;
    use std::collections::BTreeSet;

    #[test]
    fn default_topology_probe_support_matches_registered_provider_union() {
        let topology = build_default_office_integration_topology();
        let registered = topology
            .mail_providers()
            .names()
            .into_iter()
            .chain(topology.documents_providers().names())
            .chain(topology.calendar_providers().names())
            .chain(topology.contacts_directory_providers().names())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();

        let probe_supported = topology
            .probe_adapters()
            .into_iter()
            .map(|adapter| adapter.provider_kind().to_string())
            .collect::<BTreeSet<_>>();

        assert_eq!(probe_supported, registered);
    }
}

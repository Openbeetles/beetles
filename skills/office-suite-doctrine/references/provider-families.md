# Provider Families

Use this file when you need the right mental model for office provider inputs.

## Family 1: Static Secret

Representative providers:

- `imap_smtp` (including `qq`, `qqmail`, `qq_mail`)
- `feishu_mail`
- `caldav`
- `webdav`

Typical user facts:

- account identity such as `email`, `username`, or `account_id`
- password, app password, or secret
- transport facts such as host or base URL when the provider family actually needs them

Typical behavior:

- program can fill many defaults
- not every override should be asked up front
- lifecycle fields such as `refresh_token` are not the default path here

Examples:

- QQ mail usually means `imap_smtp` with `email`, password/app password, `imap_host`, and `smtp_host`
- WebDAV usually means username/account identity, password, and `base_url`

## Family 2: OAuth Refreshable

Representative providers:

- `microsoft365_mail`
- `google_mail`
- `microsoft365_calendar`
- `google_calendar`
- `microsoft365_documents`
- `google_documents`
- `microsoft365_contacts_directory`
- `google_contacts_directory`

Typical user facts:

- provider-specific OAuth result or current token material
- account identity when the provider family needs it
- optional target facts such as calendar or drive selection when the family exposes them

Lifecycle notes:

- `refresh_token`, `token_endpoint`, and expiry fields belong to auth lifecycle handling
- they are not the default first questions in the LLM mainline
- keep them in the public API surface, but do not treat them like ordinary chat-first onboarding facts

## Family 3: Tenant Token Exchange

Representative providers:

- `wecom_mail`
- `wecom_calendar`
- `wecom_documents`
- `wecom_contacts_directory`
- `feishu_calendar`
- `feishu_documents`
- `feishu_contacts_directory`

Typical user facts:

- enterprise or app identity such as `corp_id`, `app_id`, or `space_id`
- secret or token-style credential
- provider target facts such as `calendar_id` or `root_path` when required

Typical behavior:

- runtime exchanges tenant/app secrets for usable access tokens
- do not model these as generic OAuth refresh-token flows

## How To Think About Fields

- Ask for user facts first.
- Keep optional overrides available, but do not front-load them.
- Treat lifecycle fields as lifecycle fields, not as the default conversation path.
- Never ask the user or the LLM for internal fields such as `account_key` or `provider_kind` when the program can determine them.

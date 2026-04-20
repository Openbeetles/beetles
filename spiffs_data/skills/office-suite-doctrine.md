# Office Suite Doctrine

Use this skill when handling Beetle office accounts or office-backed `mail`, `calendar`, `documents`, or `contacts_directory` flows, especially when choosing between `office_status` and `office_config`, onboarding a provider, diagnosing readiness, or repairing office account routing and credentials.

## Core Doctrine

1. `office_status` is the unique office status, readiness, diagnostics, and routing-visibility entrypoint.
2. `office_config` is the complete office management/configuration/repair entrypoint.
3. Mainline `office_config` path:
   - `provider_schema`
   - `apply_account`
   - `resolve_account` only when account routing is ambiguous
4. Repair and advanced paths remain available:
   - `probe`
   - `revoke`
   - `inspect`
   - `assess`
5. Capability tools do capability work only:
   - `mail`
   - `calendar`
   - `documents`
   - `contacts_directory`
6. When capability tools are blocked by office account issues:
   - status/readiness questions go to `office_status`
   - onboarding/reconfiguration/repair go to `office_config`

## Tool Boundaries

### office_status

Use for:

- whether office accounts are configured
- whether accounts are ready right now
- diagnostics and routing visibility
- deciding whether repair is needed

Do not use it for account mutation or management operations.

### office_config

Use for:

- provider-family schema lookup
- applying account onboarding or reconfiguration
- resolving account ambiguity after configuration exists
- explicit repair and operator paths

Do not treat it as the default status tool.

### Capability tools

- `mail`: mail actions; use `office_status` for readiness and `office_config` for onboarding/repair
- `calendar`: calendar actions; use `office_status` for readiness and `office_config` for onboarding/repair
- `documents`: document actions; use `office_status` for readiness and `office_config` for onboarding/repair
- `contacts_directory`: local contacts and remote office contacts routing; use `office_status` for readiness and `office_config` for onboarding/repair

## Provider Families

### Static Secret

Representative providers:

- `imap_smtp` (including `qq`, `qqmail`, `qq_mail`)
- `feishu_mail`
- `caldav`
- `webdav`

Typical facts:

- account identity such as `email`, `username`, or `account_id`
- password, app password, or secret
- host or base URL when this provider family actually needs it

### OAuth Refreshable

Representative providers:

- `microsoft365_*`
- `google_*`

Typical facts:

- current token material or OAuth result
- account identity or target selection when the provider family requires it

Lifecycle note:

- `refresh_token`, `token_endpoint`, and expiry fields belong to auth lifecycle handling
- they stay in the public API surface, but they are not the default first questions in the LLM path

### Tenant Token Exchange

Representative providers:

- `wecom_*`
- `feishu_calendar`
- `feishu_documents`
- `feishu_contacts_directory`

Typical facts:

- enterprise or app identity such as `corp_id`, `app_id`, or `space_id`
- secret or token-style credential
- provider target facts such as `calendar_id` or `root_path` when required

## Onboarding And Repair

### Mainline

1. Use `office_status` first only if you need the current readiness picture.
2. Use `office_config(op=provider_schema)` to determine provider-family inputs.
3. Use `office_config(op=apply_account)` to configure the account.
4. Use `office_config(op=resolve_account)` only when a later capability call reports routing ambiguity.

### Repair

Use:

- `office_status` for the current picture
- `office_config(op=probe)` for explicit verification
- `office_config(op=inspect)` or `office_config(op=assess)` for advanced/operator inspection
- `office_config(op=revoke)` only when the user explicitly wants to disconnect an account

## Common Mistakes

- Starting with `office_config.inspect` for ordinary status questions
- Asking for every possible override before following the mainline
- Treating capability tools as account-setup tools
- Using `resolve_account` before configuration exists when there is no actual ambiguity

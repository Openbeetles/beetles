# Tool Boundaries

## Status vs Management

### `office_status`

Use `office_status` when the question is:

- Is this office account configured?
- Is this account ready right now?
- Which capability is blocked?
- Which account was assessed as missing facts or having runtime failure?
- Do we need repair before trying a mail, calendar, documents, or contacts action?

What it owns:

- authority summary
- readiness
- diagnostics
- account assessments
- capability routing visibility

What it does not own:

- creating or updating accounts
- revoking accounts
- manual repair operations

### `office_config`

Use `office_config` when the question is:

- Which provider family should be configured?
- What fields does this provider family require?
- Apply or update an office account
- Resolve account ambiguity after configuration exists
- Probe, revoke, inspect, or assess in repair/operator paths

What it owns:

- account onboarding
- account reconfiguration
- account repair
- provider schema lookup
- advanced/operator account management

What it does not replace:

- `office_status` for ordinary status/readiness inspection
- capability tools for actual business actions

## Capability Tools

### `mail`

Use for:

- provider mail status
- list/search/get mail
- send/draft/reply/forward

When mail is blocked by account issues:

- ask `office_status` about readiness and diagnostics
- use `office_config` for onboarding, reconfiguration, or repair

### `calendar`

Use for:

- list/get/create/update/delete events
- provider calendar status

When calendar is blocked by account issues:

- ask `office_status` about readiness and diagnostics
- use `office_config` for onboarding, reconfiguration, or repair

### `documents`

Use for:

- provider documents status
- list/read/search/summarize

When documents is blocked by account issues:

- ask `office_status` about readiness and diagnostics
- use `office_config` for onboarding, reconfiguration, or repair

### `contacts_directory`

Use for:

- local contact status/list/lookup/upsert/delete
- remote provider status for office-backed contact lookup

When remote office contacts routing is blocked by account issues:

- ask `office_status` about readiness and diagnostics
- use `office_config` for onboarding, reconfiguration, or repair

## Quick Routing Examples

- "Are my office accounts ready?" -> `office_status`
- "What provider fields do I need for QQ mail?" -> `office_config(op=provider_schema)`
- "Configure this mailbox account." -> `office_config(op=apply_account)`
- "Mail says account routing is ambiguous." -> `office_config(op=resolve_account)`
- "Why is documents unavailable?" -> `office_status`, then `office_config` only if repair is needed

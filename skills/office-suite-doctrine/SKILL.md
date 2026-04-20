---
name: office-suite-doctrine
description: Use when handling Beetle office accounts or office-backed mail, calendar, documents, or contacts flows, especially when choosing between office_status and office_config, onboarding a provider, diagnosing readiness, or repairing office account routing and credentials.
---

# Office Suite Doctrine

## Overview

This skill teaches the official Beetle office subsystem doctrine. Use it to keep status inspection, onboarding, repair, and provider-family handling aligned with the centered office authority instead of improvising a separate flow.

## When to Use

- A user wants to configure, reconnect, or revoke an office account.
- A capability tool (`mail`, `calendar`, `documents`, `contacts_directory`) is blocked by account readiness, account ambiguity, or provider routing.
- You need to decide whether to start with `office_status` or `office_config`.
- You need to choose the right factual inputs for an office provider family.

## Core Doctrine

1. Use `office_status` first for office account status, readiness, diagnostics, and routing visibility.
2. Use `office_config` for office account management. The mainline order is:
   - `provider_schema`
   - `apply_account`
   - `resolve_account` only when account routing is ambiguous
3. Keep `probe`, `revoke`, `inspect`, and `assess` for repair or advanced/operator paths. They are not the default onboarding sequence.
4. Capability tools do capability work:
   - `mail` for mail actions
   - `calendar` for calendar actions
   - `documents` for document actions
   - `contacts_directory` for contact lookups and local directory maintenance
5. When capability tools expose office-backed routing or readiness issues:
   - status/readiness questions go to `office_status`
   - onboarding, reconfiguration, and repair go to `office_config`

## Read These References When Needed

- For exact tool boundaries and which tool owns which question: [references/tool-boundaries.md](./references/tool-boundaries.md)
- For provider families and the factual inputs each family expects: [references/provider-families.md](./references/provider-families.md)
- For the onboarding mainline and repair paths: [references/onboarding-and-repair.md](./references/onboarding-and-repair.md)

## Common Mistakes

- Going to `office_config.inspect` for normal status questions instead of starting with `office_status`
- Treating capability tools as account setup tools
- Asking for every possible override up front instead of following the mainline and letting blockers ask for what is actually missing
- Using `resolve_account` as the first onboarding step instead of using it only when account routing is ambiguous

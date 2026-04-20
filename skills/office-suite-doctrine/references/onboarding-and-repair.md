# Onboarding And Repair

## Mainline Onboarding

Use this path by default:

1. `office_status` only if you first need to know what is already configured or blocked
2. `office_config(op=provider_schema)` to determine the provider-family onboarding contract
3. `office_config(op=apply_account)` to apply the account through the atomic onboarding path
4. `office_config(op=resolve_account)` only if a later capability call reports account ambiguity

What not to do:

- do not start with `inspect` for ordinary onboarding
- do not ask for every override up front
- do not use `resolve_account` before configuration exists unless the problem is actual routing ambiguity

## Repair Path

Use repair when:

- `office_status` reports readiness problems or runtime failure
- a capability tool reports missing facts, unsupported provider path, or account ambiguity
- the user explicitly wants to repair or reconnect an account

Repair tools:

- `office_status` for the current picture
- `office_config(op=probe)` for explicit verification of an existing account
- `office_config(op=inspect)` for advanced/operator inspection of persisted state
- `office_config(op=assess)` for advanced/operator assessment of stored accounts
- `office_config(op=revoke)` only when the user explicitly wants to disconnect an account

## Clarification Rules

- Missing factual inputs should come from structured blockers.
- Ask only for the facts that are still missing.
- Do not turn optional overrides into mandatory chat questions unless the current provider path actually requires them.
- Keep lifecycle fields out of the default conversation unless a provider-specific repair flow truly depends on them.

## Capability Follow-Through

After onboarding or repair:

- return to the capability tool that was blocked
- do not keep bouncing between capability tools and office tools without a clear reason
- use `office_status` again if you need to confirm readiness before retrying the capability action

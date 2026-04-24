# Configure UI Form System Design

## Goal

Unify configure-ui forms around a modern workstation visual language before
the 0.1.0 release. The work must improve the form system, not patch individual
pages with local `sx` rules.

## Visual Direction

- Use a neutral workstation surface instead of the older cream, thick-card
  form style.
- Keep page-level `SettingsSection` as the outer configuration plate, but make
  inner form modules flatter, sharper, and lower-shadow.
- Fields should read as precise control wells: clear border, restrained radius,
  stable grid rhythm, no heavy nested cards.
- Large configuration pages may use denser grids, but the global baseline stays
  calm and readable rather than dashboard-like or glass-console-like.

## Component Contract

- `FormGrid` is the single source for responsive field grids. Pages must not
  duplicate `fieldGridSx`.
- `FormActionBar` is the single form footer/action rail for grouped save or
  commit actions when the action belongs to the form body.
- `FormSwitchRow` is the standard row for binary settings with title,
  description, and control alignment.
- `FormSectionSub` and `FormSectionSubCollapsible` share the same module
  surface and header contract.
- Existing MUI inputs remain the field controls; the form system owns layout
  and grouping, not domain state.
- Mobile sub-navigation must size items by content, not by full row width, so
  the active configuration partition remains visible when switching between
  form-heavy device panels.
- Device config, account catalog/list/detail, and skill list responses must be
  normalized at the provider/type boundary before pages render. Form pages
  should not defend each field or list with local `?? ""` / `?? []` fallbacks
  because that spreads schema compatibility across the UI.
- Any form surface that can switch target identity while a request is in flight
  must use an owner/request guard before committing state. This applies to
  account filtering, account detail save/probe/delete, and provider catalog
  loading in dialogs.

## Scope

Primary migration targets:

- `AIConfigPage`
- `ChannelsConfigPage`
- `SystemConfigPage`
- `DevicePage` access forms
- `DisplayConfigPanel`
- `AudioConfigPanel`
- `HardwareGpioPanel`
- account create/detail dialogs where they use repeated form fields

Out of scope:

- P3 localStorage key migration.
- Reworking device/auth data flow beyond already committed fixes.
- Replacing MUI input behavior or introducing a new form library.

## Implementation Plan

1. Add a small pure layout contract for form grids and test it first.
2. Add `FormGrid`, `FormActionBar`, and `FormSwitchRow` in
   `src/components/form`.
3. Modernize form module style constants in `src/theme/panelStyles.ts`.
4. Replace duplicated local `fieldGridSx` in the primary config panels.
5. Replace high-impact binary rows with `FormSwitchRow`.
6. Update design docs with the new rules.
7. Verify with focused tests, lint, and build; perform browser visual checks
   on representative pages.

## Release Criteria

- No duplicated page-level `fieldGridSx` remains in migrated pages.
- The changed paths pass targeted tests, lint, and production build with no
  warnings.
- Visual checks cover at least AI config, channels config, audio config, and
  display/hardware config panels.

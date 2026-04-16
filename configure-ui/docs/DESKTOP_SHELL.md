# Configure UI Desktop Shell

## Scope

`configure-ui` keeps the existing React/Vite product behavior as the single source of truth.
The Tauri layer only wraps that UI as a desktop app and adds desktop runtime capabilities.
It must not fork product behavior away from the browser version.

## Version Source

- Single source: `configure-ui/package.json`
- Synced targets:
  - `configure-ui/src-tauri/tauri.conf.json`
  - `configure-ui/src-tauri/Cargo.toml`
- Sync command:

```bash
cd configure-ui
npm run tauri:sync-version
```

- CI check:

```bash
cd configure-ui
npm run tauri:sync-version:check
```

The `pretauri` hook runs the sync automatically before `npm run tauri ...`.

## Runtime Baseline

- **Single instance**: the desktop shell should only allow one running instance. A second launch focuses the existing main window.
- **Window state restore**: size and position restore across launches via `tauri-plugin-window-state`.
- **Platform detection**: frontend runtime checks live under `src/runtime/`, not scattered through pages/components.
- **No unused native plugins**: only keep Rust/Tauri plugins that are actually used by the current shell.

## CI / Release

- CI workflow: `.github/workflows/configure-ui-desktop.yml`
  - Verifies version sync
  - Runs desktop base tests
  - Runs lint + web build
  - Runs `cargo check` for the shell
  - Produces debug desktop bundles on macOS / Windows / Linux
- Tagged repo release: `.github/workflows/release.yml`
  - Verifies `configure-ui/package.json` matches the tagged repo version
  - Builds release desktop bundles on macOS / Windows / Linux
  - Attaches desktop artifacts to the repo release bundle

## Current Security Posture

- The shell currently wraps the existing local-network configuration UI, so `csp` remains `null` for compatibility with device-address access patterns.
- New native capabilities must be added intentionally through Tauri plugins / commands and corresponding capabilities, not ad-hoc frontend checks.

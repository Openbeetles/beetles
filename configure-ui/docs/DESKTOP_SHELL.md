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

## Visual Baseline

- **Desktop shell direction**: use a clean desktop-OS material language, not mixed web glass + pseudo-skeuomorphic decoration.
- **Single light model**: chrome, content plates, wells, and controls should share the same top-light / contact-shadow logic from `src/theme/os3dLanguage.ts`.
- **Material hierarchy**: background < shell chrome < content plates < raised controls. Avoid inventing one-off shadow recipes in page components.
- **Diffuse first**: light-mode skeuomorphism should read through long-radius ambient shadows with low alpha, not short hard stacks. If a surface edge feels outlined before it feels cushioned, the shadow recipe is still too hard.
- **Wide wax highlights**: buttons, cards, and input wells should use broad, low-contrast top highlights rather than thin bright lips. If the top edge reads as a line before it reads as a surface roll-off, the material is still too sharp.
- **Components must not re-harden tokens**: after softening `os3dLanguage` shadow stacks, component overrides (`MuiButton`, `MuiCard`, `MuiOutlinedInput`, toggle groups) must also reduce border contrast, hover lift, and highlight intensity. Otherwise the app still reads like hard web cards on top of soft tokens.
- **Prefer highlights over borders**: thin top highlights and contact shadows should do most of the shaping work; borders stay subtle.
- **Start menu / launch panels**: treat them as elevated floating boards, not tooltip popovers; use a dedicated shadow stack and a visible air gap above the taskbar.
- **Popover spacing**: for MUI start panels, keep the taskbar gap via layout offset instead of `transform`, because the built-in `Grow` transition owns the paper transform in both web and desktop shells.
- **Dock motion**: bottom taskbar icons should use a restrained macOS-style magnification curve where the hovered icon leads and immediate neighbors follow lightly; avoid tooltip-like jumps or full-card bounce.
- **Dock geometry**: the center dock row should sit on the taskbar baseline, not in its vertical middle; reserve overflow above the bar so magnified icons can emerge past the chrome edge instead of being visually swallowed by it.
- **Dock layering**: the center dock row should live on its own overlay plane above the taskbar, not as a normal flex child inside the bar height. Otherwise hover magnification always reads as clipped even when `overflow` is technically visible.
- **Dock DOM split**: keep overlay positioning and icon-row layout in separate nodes. The overlay wrapper handles centering and hit area boundaries; the icon row itself must stay `overflow: visible` and must not own horizontal scrolling.
- **Dock baseline and spacing**: in idle state, the icon row should be vertically centered against the taskbar height. Magnification must preserve visible air between icons; if scale eats the gap, reduce the curve before pushing icons apart with hacks.
- **Home-page spacing**: top/bottom breathing room for the disconnected setup view belongs to the scroll canvas wrapper, not to card `margin-top` hacks; this preserves edge padding without introducing phantom spacer bands or broken vertical scrolling.
- **Home-page scroll edges**: the device home scroll canvas must own the vertical page padding for both disconnected and connected states. Do not rely on per-card margins or disconnected-only wrappers, or the connected dashboard will still stick to the top and bottom edges while scrolling.
- **Shell content insets**: page distance from the header and taskbar belongs to the shared `MainSurface` content slot padding, not to empty spacer nodes and not to one page adding its own ad-hoc `py`. Single-page padding should only exist when that page needs extra breathing beyond the shell baseline.
- **Immersive page exception**: the `/device` home/dashboard is an immersive shell page and must not inherit the default `MainSurface` top/bottom inset. Standard config/list pages keep the shared shell inset; the dashboard opts out explicitly instead of fighting it with per-page negative spacing.
- **Immersive dashboard spacing**: opting `/device` out of the shared shell inset does not mean zero breathing room. The dashboard must restore its own top/bottom spacing on `data-app-scroll-region`, so both the idle view and scroll edges keep air without reintroducing shell spacer bands.
- **Chrome seams**: only one layer should define the top/content seam. If the titlebar already owns the divider, the main surface must not add a second inset lip, or it reads as a blurry spacer band.
- **No shell spacer bands**: do not insert fixed-height empty boxes between titlebar/banner, main surface, and taskbar just to manufacture “breathing room”. Vertical breathing belongs to page scroll canvases, otherwise the shell itself creates permanent blank bands at the top and bottom of every page.

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

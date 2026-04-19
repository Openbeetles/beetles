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
- **Natural-light translucency over heavy volume**: Beetle OS skeuomorphism should feel like soft daylight gliding over paper, not thick acrylic blocks. Prefer fewer shadow layers, broader highlight roll-off, and larger flowing radii before adding more lift, glow, or edge contrast.
- **Components must not re-harden tokens**: after softening `os3dLanguage` shadow stacks, component overrides (`MuiButton`, `MuiCard`, `MuiOutlinedInput`, toggle groups) must also reduce border contrast, hover lift, and highlight intensity. Otherwise the app still reads like hard web cards on top of soft tokens.
- **Controls should feel desktop, not SaaS**: outlined buttons, input wells, popovers, dialogs, and toggle groups should stay low-contrast and cushioned. If they read like glossy web controls dropped onto a soft shell, the material system is still split.
- **Compact segmented filters stay thin**: small `ToggleButtonGroup` filters should read as an inset tray with fitted labels, not as a stack of thick bordered pills. Their row height should stay aligned with adjacent `size="small"` buttons; if the filter chips become taller than nearby small actions, the control is overbuilt.
- **Manager lists need hierarchy**: object-management pages such as Skills or Accounts must not expose raw identifiers as the primary headline. Lead with a readable label, demote the raw id to secondary monospace text, and group row actions into a compact control cluster so the page reads like an OS manager, not a debug dump.
- **Wide manager rows must use the width**: on desktop-width manager pages, do not leave a single text column stretched across a full plate with only an orphan chevron on the far edge. Split account / skill / object rows into aligned information rails so state, metadata, and entry affordance each occupy deliberate space.
- **Manager cards need an obvious focal hierarchy**: when a list moves from rows to cards, the card still needs one clear anchor. Lead with a readable title, keep provider/type as an eyebrow, reserve the loudest module for runtime/readiness state, and demote raw ids to the footer rail. If chips and metadata compete with the title, the card has lost its focal point.
- **Manager metadata stays fitted**: repeated structural labels such as `Runtime` / `Custom` belong as a small inline badge beside the item title, not as a dedicated first line that burns vertical space and overwhelms row rhythm.
- **Metadata badges align on a rail**: when manager cards show a structural badge and a raw id, keep them on the same secondary metadata row so the badge position stays consistent across items. Do not let a tiny badge trail after variable-length titles and create a ragged edge.
- **Action grouping uses structure, not separator lines**: list-row edit/delete actions may share a compact tool tray, but state toggles should read as a sibling control with spacing and alignment. Do not fake hierarchy by dropping a vertical divider into one oversized button box.
- **Destructive actions need destructive color**: inline remove/delete actions must not share the same icon color as neutral edit actions. Keep edit tools quiet, and give destructive tools a restrained danger tint so the action is legible before hover.
- **Profile/personality option groups use fitted parts**: chips and radio choices inside settings forms should read as compact OS option pieces with shared wells and selected-pill lift. Avoid saturated brand fills and naked text rows that fall back to generic web form styling.
- **Text tiers must be real tiers**: `--text-secondary` and `--text-tertiary` cannot collapse to the same value. Secondary copy carries supporting content; tertiary is reserved for metadata, ids, and de-emphasized labels. If both read at the same contrast, the whole page goes visually flat.
- **When semantics are open-ended, prefer monograms over fake icon meaning**: manager lists must not repeat one fallback icon for every row, but they also should not invent brittle semantic mappings that only cover part of the dataset. If the API lacks stable icon metadata, use a consistent 3D monogram badge derived from the item label so each row gets identity without lying about meaning.
- **Settings subgroups are modules, not sticky web strips**: secondary sections inside config pages should read as compact control modules with a light header bar and a quieter body well. Do not fall back to translucent sticky headers that feel like browser docs.
- **Selection should feel like a fitted part**: subnav active rows, launcher tiles, and selected toggles should read as inserted OS pieces with a small primary cue and a single shared lift recipe. Avoid flat color wash or oversized glows.
- **Empty states are not alerts**: neutral empty lists and missing-content placeholders should use a centered, quiet system-empty composition. Reserve row-oriented strips, semantic accent edges, and warning tone for notices/errors only.
- **Unsupported endpoints are not failures**: when a device build simply does not expose a page API, render a neutral unsupported-state block inside the page instead of surfacing raw `not found` errors in danger styling.
- **Disconnected is not a fetch failure**: if the device is unreachable or the URL is not filled in, gate page autoload and show a calm connect-first state. Do not fire page loads just to paint `Failed to fetch` above an empty or half-dead layout.
- **One dataset, one request chain**: route pages with both mount sync and manual refresh should keep a single shared loader per dataset, then vary only notification policy. Do not duplicate the same request bundle in `useEffect` and `reload*` callbacks and let them drift.
- **Repeated editors share one form body**: if a route needs the same connection/configuration editor in both a setup hero and an embedded dashboard card, keep one shared editor component and let only the outer shell differ.
- **Config pages share one editor controller**: settings pages should not each hand-roll the same autoload, dirty, save-feedback, and save-success cleanup flow. Use the shared config editor controller, and keep page code focused on domain state and layout.
- **Validators stay pure and out of JSX pages**: config validation rules belong in dedicated pure modules beside the page, not inline inside the render/controller file. Pages may compose validators, but they should not grow long `if/return` validation ladders next to save logic and JSX.
- **Route-owned setup pages own their hint state**: if a page is itself the setup/connection destination, do not repeat the same global warning ribbon above it. The page body should carry that state once.
- **Detail dialogs anchor commit actions**: long account/config detail dialogs should use the same scroll-well + fixed-footer structure as create/edit dialogs, so save/delete stay on the bottom rail while the body scrolls independently.
- **Diagnostic actions stay with diagnostics**: probe/test actions belong beside assessment/runtime content, not in the same footer row as save/delete. Footer rows are for durable commit/destructive actions; diagnostics stay contextual.
- **Shell preferences belong to the shell chrome, not the title headline**: app-level theme/language preferences should live in the taskbar / start / tray area, not as a floating top-right titlebar button that competes with page actions.
- **Tray controls share one baseline**: taskbar tray actions and status chips must sit on the same height rail. Do not mix a tall square pedestal with a shorter pill next to it.
- **Repeated feedback must replay**: transient shell feedback such as toast/snackbar notices must spawn a fresh instance per event. Re-triggering the same message should still surface again without requiring a page refresh.
- **Desktop titlebars stay thin**: desktop shells, especially macOS Tauri windows, must reserve the traffic-light lane only once. Do not stack extra titlebar height and extra top padding on top of the same native control clearance.
- **macOS titlebars follow the traffic-light rail**: on macOS Tauri windows, the titlebar should yield the first lane to the traffic lights, then show a single compact page 3D icon on that same row. Surface the page title via hover tooltip instead of a second text chip/control, keep the caption content on its own compact rail instead of vertically centering it inside the whole bar, and align the icon by visual center rather than trusting PNG whitespace or regular inline shadows.
- **Prefer highlights over borders**: thin top highlights and contact shadows should do most of the shaping work; borders stay subtle.
- **Loading shells stay flat**: first-paint / refresh loading states must not reuse the same raised card shadow stack as ready content. Keep loading surfaces visually quiet; only data-ready plates earn `CONFIG_PANEL_SX` / dashboard card lift.
- **Boot fallback owns the viewport**: route-level lazy fallback must cover the full viewport with a calm background. Do not leave a short spinner box over the global page backdrop, or refresh flashes expose shell-less bands before layout mounts.
- **Start menu / launch panels**: treat them as elevated floating boards, not tooltip popovers; use a dedicated shadow stack and a visible air gap above the taskbar.
- **One shell dialect**: the taskbar, dock, and launcher must read as one desktop language. Do not mix macOS-style dock motion with Windows/Metro-style variable tile sizing inside the same shell; launcher grids should stay calm, regular, and icon-led.
- **Popover spacing**: for MUI start panels, keep the taskbar gap via layout offset instead of `transform`, because the built-in `Grow` transition owns the paper transform in both web and desktop shells.
- **Dock motion**: bottom taskbar icons should use a restrained macOS-style magnification curve where the hovered icon leads and immediate neighbors follow lightly; avoid tooltip-like jumps or full-card bounce.
- **Dock geometry**: the center dock row should sit on the taskbar baseline, not in its vertical middle; reserve overflow above the bar so magnified icons can emerge past the chrome edge instead of being visually swallowed by it.
- **Dock layering**: the center dock row should live on its own overlay plane above the taskbar, not as a normal flex child inside the bar height. Otherwise hover magnification always reads as clipped even when `overflow` is technically visible.
- **Dock DOM split**: keep overlay positioning and icon-row layout in separate nodes. The overlay wrapper handles centering and hit area boundaries; the icon row itself must stay `overflow: visible` and must not own horizontal scrolling.
- **Dock baseline and spacing**: in idle state, the icon row should be vertically centered against the taskbar height. Magnification must preserve visible air between icons; if scale eats the gap, reduce the curve before pushing icons apart with hacks.
- **Quiet chrome**: titlebars should not compete with page content. Brand affordances, page titles, and caption actions must stay quieter than launcher surfaces and quieter than the page body; avoid decorative underline accents in shell chrome.
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

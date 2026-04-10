# Device Dashboard Redesign (Bento Box Style)

## 1. Overview
The goal of this redesign is to transform the existing `DevicePage` and `SystemStatusPanel` in `@configure-ui` into a high-end, modern dashboard using a "Bento Box" layout. This style emphasizes visual hierarchy, clean lines, and data density without clutter, aligning with the project's existing CSS variables and Tailwind-like utility classes.

## 2. Architecture & Components

### 2.1 Top Bar (Connection Control)
- **Current State:** A standard form layout with text fields and buttons taking up significant vertical space.
- **New Design:** A compact, pill-shaped or minimalist floating toolbar. It will use a subtle background (`color-mix(in srgb, var(--foreground) 2%, transparent)`) to distinguish it from the main dashboard content without drawing too much attention.

### 2.2 Hero Bento Grid (Top Section)
The primary visual focus of the dashboard.
- **Device Identity Card:** A large card spanning 2 columns (or full width on mobile) displaying the Product Name, IP Address, Firmware Version, and Board ID. It will feature a subtle background gradient or a large watermark icon to create a premium feel.
- **System Health & Pressure Card:** A medium card displaying the current pressure state (Normal/Cautious/Critical) with a prominent colored indicator (e.g., a glowing dot or large colored text) and WiFi connection status.
- **Storage Card:** A medium card replacing the thin linear progress bar with a thicker, rounded linear progress bar or a semi-circle chart. The percentage and used/total text will be enlarged and styled with a monospaced font (`var(--font-mono)`).

### 2.3 Secondary Bento Grid (Middle Section)
- **System Resources:** A grid of smaller cards (or distinct sections within a larger card) for Heap (Internal/SPIRAM), Active HTTP connections, and Session count. Numbers will be large and bold, with muted labels.
- **Traffic & Operations:** Grouped metrics for Messages In/Out, LLM Calls, Tool Calls, and Dispatch Success/Fail. Icons will be added to these metrics to improve scannability.
- **Channel Connectivity:** The existing `ChannelConnectivityPanel` will be integrated into this grid as a standalone card, matching the border radius and shadow of the other Bento boxes.

### 2.4 Tertiary Bento Grid (Bottom Section)
- **Errors & WiFi Anomalies:** A dedicated section for error counters (Router, Context, Tool Exec, LLM, etc.) and WiFi reconnects. To keep the UI clean, zero-value errors will be visually muted or hidden, while non-zero values will be highlighted with `var(--semantic-danger)`.
- **Last Error:** A full-width card at the bottom. If an error exists, it will feature a red accent line (`border-left`) and a tinted background. If no error exists, it will remain subtle and muted.

## 3. Styling & Theming Guidelines
- **CSS Variables:** Extensive use of `var(--card)`, `var(--radius-card)`, `var(--radius-chip)`, `var(--border)`, `var(--border-subtle)`, `var(--shadow-subtle)`, `var(--foreground)`, `var(--muted)`, and `var(--primary)`.
- **Tailwind Variables:** Use Tailwind-compatible spacing and typography variables where applicable (e.g., `var(--font-size-data-value)`, `var(--font-mono)`).
- **Typography:** Data values will use `fontVariantNumeric: "tabular-nums"` and `var(--font-mono)` for alignment and a tech-forward look. Labels will use `var(--font-size-caption)` with increased letter spacing (`letterSpacing: "0.02em"`).
- **Hover Effects:** Cards will feature subtle hover states (e.g., slightly darker border or subtle shadow increase) to make the dashboard feel interactive.

## 4. Data Flow & State Management
- No changes to the underlying data fetching logic (`useDeviceApi`, `fetchSystemInfoCoalesced`, `api.system.health`, etc.).
- The redesign is purely presentational, affecting `DevicePage.tsx` and `SystemStatusPanel.tsx`.

## 5. Error Handling
- The existing `InlineAlert` and `SectionLoadProgress` components will be retained but styled to fit the new Bento layout.
- Network errors during data fetching will continue to display inline retry prompts.

## 6. Testing Strategy
- Verify responsive behavior across mobile (1 column), tablet (2 columns), and desktop (3-4 columns) breakpoints.
- Ensure dark mode compatibility by relying strictly on the existing CSS variables.
- Verify that error states (e.g., non-zero error counts, `last_error` present) correctly apply the `var(--semantic-danger)` styling.
import Box from "@mui/material/Box";
import type { BoxProps } from "@mui/material/Box";
import type { SxProps, Theme } from "@mui/material/styles";
import { useId } from "react";

const wingAndCoreSx = {
  "& .wing-left": {
    transformOrigin: "50% 35%",
    animation: "beetle-wing-flap-left 0.8s ease-in-out infinite alternate",
  },
  "& .wing-right": {
    transformOrigin: "50% 35%",
    animation: "beetle-wing-flap-right 0.8s ease-in-out infinite alternate",
  },
  "& .core-glow": {
    animation: "beetle-pulse-glow 1.5s ease-in-out infinite alternate",
  },
} as const;

const reducedMotionSx = {
  "@media (prefers-reduced-motion: reduce)": {
    "& .wing-left": { animation: "none" },
    "& .wing-right": { animation: "none" },
    "& .core-glow": { animation: "none" },
  },
} as const;

/**
 * 甲壳虫品牌标（与 beetle_site `CyberBeetleMark` 同源图形），随 `var(--primary)` / 表面色变化。
 * Cyber-beetle mark (same artwork as beetle_site); tints follow theme CSS variables.
 */
export function BeetleIcon({
  sx,
  ...rest
}: { sx?: SxProps<Theme> } & Omit<BoxProps, "sx">) {
  const uid = useId().replace(/:/g, "");
  const bodyGradId = `cb-body-${uid}`;
  const wingGradId = `cb-wing-${uid}`;

  return (
    <Box
      sx={{
        display: "block",
        flexShrink: 0,
        width: "var(--icon-container-md)",
        height: "var(--icon-container-md)",
        position: "relative",
        ...wingAndCoreSx,
        ...reducedMotionSx,
        ...sx,
      }}
      {...rest}
    >
      <svg
        viewBox="0 0 100 100"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        aria-hidden
        style={{
          width: "100%",
          height: "100%",
          filter:
            "drop-shadow(0 0 8px color-mix(in srgb, var(--primary) 35%, transparent))",
        }}
      >
        <defs>
          <linearGradient id={bodyGradId} x1="0" y1="0" x2="0" y2="1">
            <stop
              offset="0%"
              stopColor="color-mix(in srgb, var(--foreground) 14%, var(--card))"
            />
            <stop
              offset="100%"
              stopColor="color-mix(in srgb, var(--foreground) 30%, var(--card))"
            />
          </linearGradient>
          <linearGradient id={wingGradId} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor="var(--primary)" stopOpacity="0.8" />
            <stop offset="100%" stopColor="var(--primary)" stopOpacity="0.1" />
          </linearGradient>
        </defs>

        <path
          d="M40 30 L15 15 M60 30 L85 15 M35 50 L10 50 M65 50 L90 50 M40 70 L20 90 M60 70 L80 90"
          stroke="color-mix(in srgb, var(--primary) 40%, transparent)"
          strokeWidth="2"
          strokeLinecap="round"
        />

        <path
          d="M50 85 C35 85, 30 65, 30 45 C30 25, 40 15, 50 15 C60 15, 70 25, 70 45 C70 65, 65 85, 50 85 Z"
          fill={`url(#${bodyGradId})`}
          stroke="var(--primary)"
          strokeWidth="1.5"
        />

        <g opacity="0.85">
          <rect
            x="38"
            y="25"
            width="24"
            height="46"
            rx="2"
            fill="color-mix(in srgb, var(--foreground) 90%, var(--card))"
            stroke="var(--primary)"
            strokeWidth="0.5"
          />
          <path
            d="M38 30 h-3 M38 34 h-3 M38 38 h-3 M38 42 h-3 M38 52 h-3 M38 56 h-3 M38 60 h-3 M38 64 h-3"
            stroke="var(--primary)"
            strokeWidth="0.5"
          />
          <path
            d="M62 30 h3 M62 34 h3 M62 38 h3 M62 42 h3 M62 52 h3 M62 56 h3 M62 60 h3 M62 64 h3"
            stroke="var(--primary)"
            strokeWidth="0.5"
          />
          <path
            d="M35 30 L30 30 L25 25"
            fill="none"
            stroke="var(--primary)"
            strokeWidth="0.5"
          />
          <circle cx="25" cy="25" r="0.8" fill="var(--primary)" />
          <path
            d="M35 64 L30 64 L25 69"
            fill="none"
            stroke="var(--primary)"
            strokeWidth="0.5"
          />
          <circle cx="25" cy="69" r="0.8" fill="var(--primary)" />
          <path
            d="M65 30 L70 30 L75 25"
            fill="none"
            stroke="var(--primary)"
            strokeWidth="0.5"
          />
          <circle cx="75" cy="25" r="0.8" fill="var(--primary)" />
          <path
            d="M65 64 L70 64 L75 69"
            fill="none"
            stroke="var(--primary)"
            strokeWidth="0.5"
          />
          <circle cx="75" cy="69" r="0.8" fill="var(--primary)" />
          <text
            x="50"
            y="32"
            fontSize="3.5"
            fill="var(--primary)"
            opacity="0.9"
            textAnchor="middle"
            fontFamily="monospace"
            letterSpacing="0.5"
            fontWeight="bold"
          >
            BTL-01
          </text>
          <text
            x="50"
            y="66"
            fontSize="2.5"
            fill="var(--primary)"
            opacity="0.7"
            textAnchor="middle"
            fontFamily="monospace"
          >
            SYS.ON
          </text>
          <path
            d="M42 20 L58 20"
            stroke="var(--primary)"
            strokeWidth="0.5"
            strokeDasharray="1 2"
          />
          <path
            d="M42 75 L58 75"
            stroke="var(--primary)"
            strokeWidth="0.5"
            strokeDasharray="1 2"
          />
          <circle cx="50" cy="20" r="1" fill="var(--primary)" />
          <circle cx="50" cy="75" r="1" fill="var(--primary)" />
        </g>

        <path
          d="M42 15 C 40 7, 46 4, 50 4 C 54 4, 60 7, 58 15 Z"
          fill="color-mix(in srgb, var(--foreground) 90%, var(--card))"
          stroke="var(--primary)"
          strokeWidth="1"
        />
        <path
          d="M46 6 Q 44 2 40 2 M54 6 Q 56 2 60 2"
          fill="none"
          stroke="var(--primary)"
          strokeWidth="0.8"
          strokeLinecap="round"
        />
        <circle
          cx="45"
          cy="10"
          r="1.5"
          fill="var(--primary)"
          className="core-glow"
        />
        <circle
          cx="55"
          cy="10"
          r="1.5"
          fill="var(--primary)"
          className="core-glow"
        />

        <rect
          x="42"
          y="37"
          width="16"
          height="16"
          rx="2"
          fill="color-mix(in srgb, var(--foreground) 90%, var(--card))"
          stroke="var(--primary)"
          strokeWidth="1"
        />
        <circle
          cx="50"
          cy="45"
          r="5"
          fill="none"
          stroke="var(--primary)"
          strokeWidth="0.5"
          strokeDasharray="1 1"
        />
        <circle
          cx="50"
          cy="45"
          r="3"
          fill="var(--primary)"
          className="core-glow"
        />
        <path
          d="M50 53 L50 80"
          stroke="var(--primary)"
          strokeWidth="0.5"
          strokeDasharray="2 2"
        />

        <g className="wing-left">
          <path
            d="M50 35 C25 25, 5 50, 15 80 C25 90, 45 75, 48 50 Z"
            fill={`url(#${wingGradId})`}
            stroke="var(--primary)"
            strokeWidth="1"
          />
          <g
            stroke="color-mix(in srgb, var(--primary) 70%, transparent)"
            strokeWidth="0.5"
            fill="none"
          >
            <path d="M45 45 L35 45 L25 55 L22 55" />
            <circle
              cx="22"
              cy="55"
              r="0.8"
              fill="var(--primary)"
              stroke="none"
            />
            <path d="M45 52 L38 52 L30 60 L25 60" />
            <circle
              cx="25"
              cy="60"
              r="0.8"
              fill="var(--primary)"
              stroke="none"
            />
            <path d="M45 59 L40 59 L32 67 L28 67" />
            <circle
              cx="28"
              cy="67"
              r="0.8"
              fill="var(--primary)"
              stroke="none"
            />
            <path
              d="M18 70 L20 68 L23 68 L25 70 L23 72 L20 72 Z"
              strokeWidth="0.3"
            />
          </g>
        </g>

        <g className="wing-right">
          <path
            d="M50 35 C75 25, 95 50, 85 80 C75 90, 55 75, 52 50 Z"
            fill={`url(#${wingGradId})`}
            stroke="var(--primary)"
            strokeWidth="1"
          />
          <g
            stroke="color-mix(in srgb, var(--primary) 70%, transparent)"
            strokeWidth="0.5"
            fill="none"
          >
            <path d="M55 45 L65 45 L75 55 L78 55" />
            <circle
              cx="78"
              cy="55"
              r="0.8"
              fill="var(--primary)"
              stroke="none"
            />
            <path d="M55 52 L62 52 L70 60 L75 60" />
            <circle
              cx="75"
              cy="60"
              r="0.8"
              fill="var(--primary)"
              stroke="none"
            />
            <path d="M55 59 L60 59 L68 67 L72 67" />
            <circle
              cx="72"
              cy="67"
              r="0.8"
              fill="var(--primary)"
              stroke="none"
            />
            <path
              d="M82 70 L80 68 L77 68 L75 70 L77 72 L80 72 Z"
              strokeWidth="0.3"
            />
          </g>
        </g>
      </svg>
    </Box>
  );
}

import { useId } from "react";
import Box from "@mui/material/Box";

const PIN_Y = [124, 136, 148, 160, 172, 184, 196] as const;

export type PcbDecorTone = "default" | "embed";

/**
 * 主内容区装饰：原理图走线 + QFP 封装（丝印框、Pin1、焊盘芯）+ 辅 SOIC，随主题色变化。
 * Schematic traces + QFP silk (courtyard, pin-1, die pad) + secondary SOIC; uses theme tokens only.
 *
 * @param tone `embed`：卡片内嵌时压低对比，避免与主表面 `MAIN_SURFACE_PCB` 叠成「双层 PCB」抢正文。
 */
export function PcbDecorOverlay({ tone = "default" }: { tone?: PcbDecorTone }) {
  const uid = useId().replace(/:/g, "");
  const tracePat = `pcb-t-${uid}`;
  const chipPat = `pcb-c-${uid}`;
  const embedDim = tone === "embed" ? 0.44 : 1;

  return (
    <Box
      aria-hidden
      sx={{
        position: "absolute",
        inset: 0,
        zIndex: 0,
        pointerEvents: "none",
        overflow: "hidden",
        opacity: embedDim,
        "& svg": { display: "block" },
      }}
    >
      {/* 走线 / 原理图 */}
      <svg
        width="100%"
        height="100%"
        xmlns="http://www.w3.org/2000/svg"
        style={{
          position: "absolute",
          inset: 0,
          color:
            "color-mix(in srgb, var(--foreground) 14%, transparent)",
          opacity: 0.94,
        }}
      >
        <defs>
          <pattern
            id={tracePat}
            width={360}
            height={360}
            patternUnits="userSpaceOnUse"
          >
            <g
              fill="none"
              stroke="currentColor"
              strokeWidth={1}
              strokeLinecap="square"
              strokeLinejoin="miter"
            >
              <path d="M 0 92 L 88 92 L 88 158 L 172 158 L 172 52 L 252 52" />
              <path d="M 360 68 L 232 68 L 232 208 L 148 208" />
              <path d="M 48 360 L 48 276 L 132 276 L 132 196" />
              <path d="M 268 360 L 268 288 L 360 288" />
              <path d="M 118 24 L 118 0" />
              <path d="M 200 0 L 200 36" />
              <circle cx={200} cy={124} r={2} fill="currentColor" />
              <path d="M 200 124 L 200 98" />
              <path d="M 200 124 L 222 124" />
              <path
                d="M 78 236 h 6 l 4 5 l -6 5 l 6 5 l -4 5 h 6"
                strokeWidth={0.85}
              />
              <path d="M 278 132 v 20 M 292 132 v 20 M 278 142 h 14" />
              <path
                d="M 300 48 l 8 8 m -8 0 l 8 -8"
                strokeWidth={0.75}
                opacity={0.75}
              />
            </g>
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill={`url(#${tracePat})`} />
      </svg>
      {/* 芯片封装层：偏 primary */}
      <svg
        width="100%"
        height="100%"
        xmlns="http://www.w3.org/2000/svg"
        style={{
          position: "absolute",
          inset: 0,
          color: "color-mix(in srgb, var(--primary) 15%, transparent)",
          opacity: 0.88,
        }}
      >
        <defs>
          <pattern
            id={chipPat}
            width={360}
            height={360}
            patternUnits="userSpaceOnUse"
          >
            <g
              fill="none"
              stroke="currentColor"
              strokeLinecap="square"
              strokeLinejoin="miter"
            >
              {/* 辅 SOIC：左上小区块，打破平铺单调 */}
              <g opacity={0.45} strokeWidth={0.65}>
                <rect x={24} y={248} width={52} height={22} rx={2} />
                <line x1={20} y1={254} x2={24} y2={254} />
                <line x1={20} y1={259} x2={24} y2={259} />
                <line x1={20} y1={264} x2={24} y2={264} />
                <line x1={76} y1={254} x2={80} y2={254} />
                <line x1={76} y1={259} x2={80} y2={259} />
                <line x1={76} y1={264} x2={80} y2={264} />
                <circle cx={40} cy={259} r={1.2} fill="currentColor" />
              </g>

              {/* QFP 丝印 courtyard（虚线框，略大于管体） */}
              <rect
                x={108}
                y={108}
                width={104}
                height={104}
                rx={3}
                strokeWidth={0.45}
                strokeDasharray="3 5"
                opacity={0.55}
              />

              {/* 管体：顶边 Pin1 侧一切角（常见 QFP 丝印） */}
              <path
                d="M 124 116 L 199 116 A 5 5 0 0 1 204 121 L 204 199 A 5 5 0 0 1 199 204 L 121 204 A 5 5 0 0 1 116 199 L 116 124 L 124 116 Z"
                strokeWidth={1.05}
              />

              {/* Pin1 圆点（切角侧） */}
              <circle cx={121} cy={121} r={2.2} fill="currentColor" />

              {/* 内腔（die attach / 散热焊盘示意） */}
              <rect
                x={132}
                y={132}
                width={56}
                height={56}
                rx={2}
                strokeWidth={0.7}
                opacity={0.62}
              />
              <path
                d="M 160 142 V 178 M 142 160 H 178"
                strokeWidth={0.5}
                strokeDasharray="2 2"
                opacity={0.5}
              />
              {/* 中心 stipple：模拟散热过孔栅 */}
              <g fill="currentColor" opacity={0.35}>
                <circle cx={148} cy={148} r={0.65} />
                <circle cx={160} cy={148} r={0.65} />
                <circle cx={172} cy={148} r={0.65} />
                <circle cx={148} cy={160} r={0.65} />
                <circle cx={172} cy={160} r={0.65} />
                <circle cx={148} cy={172} r={0.65} />
                <circle cx={160} cy={172} r={0.65} />
                <circle cx={172} cy={172} r={0.65} />
              </g>

              {/* 四边引脚 */}
              {PIN_Y.map((y) => (
                <line
                  key={`L${y}`}
                  x1={108}
                  y1={y}
                  x2={116}
                  y2={y}
                  strokeWidth={0.95}
                />
              ))}
              {PIN_Y.map((y) => (
                <line
                  key={`R${y}`}
                  x1={204}
                  y1={y}
                  x2={212}
                  y2={y}
                  strokeWidth={0.95}
                />
              ))}
              {PIN_Y.map((x) => (
                <line
                  key={`T${x}`}
                  x1={x}
                  y1={108}
                  x2={x}
                  y2={116}
                  strokeWidth={0.95}
                />
              ))}
              {PIN_Y.map((x) => (
                <line
                  key={`B${x}`}
                  x1={x}
                  y1={204}
                  x2={x}
                  y2={212}
                  strokeWidth={0.95}
                />
              ))}

              <text
                x={160}
                y={114}
                textAnchor="middle"
                fontSize={5}
                fontFamily="var(--font-mono), monospace"
                fill="currentColor"
                stroke="none"
                opacity={0.4}
              >
                U1
              </text>
            </g>
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill={`url(#${chipPat})`} />
      </svg>
    </Box>
  );
}

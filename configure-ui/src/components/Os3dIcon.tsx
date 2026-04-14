import Box from "@mui/material/Box";
import type { SxProps, Theme } from "@mui/material/styles";

const IMG_SX: SxProps<Theme> = {
  width: "100%",
  height: "100%",
  objectFit: "contain",
  filter:
    "drop-shadow(0 4px 6px rgba(0, 0, 0, 0.15)) drop-shadow(0 1px 3px rgba(0, 0, 0, 0.1))",
  transition: "transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1)",
  "@media (prefers-reduced-motion: reduce)": {
    transition: "none",
  },
};

/**
 * 拟物 3D PNG 图标（与任务栏 `OsIcon` 同滤镜）；父级需给定尺寸（如 `icon-size-lg` 容器）。
 * 默认作装饰图：`aria-hidden`，`alt` 为空（若需读屏文案请设 `decorative={false}` 并给 `alt`）。
 */
export function Os3dIcon({
  src,
  alt = "",
  decorative = true,
  sx,
}: {
  src: string;
  alt?: string;
  /** 装饰性（无独立语义）：默认隐藏于读屏 */
  decorative?: boolean;
  sx?: SxProps<Theme>;
}) {
  return (
    <Box
      component="img"
      src={src}
      alt={decorative ? "" : alt}
      draggable={false}
      decoding="async"
      aria-hidden={decorative ? true : undefined}
      sx={sx != null ? ([IMG_SX, sx] as SxProps<Theme>) : IMG_SX}
    />
  );
}

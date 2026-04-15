import Box from "@mui/material/Box";
import type { SxProps, Theme } from "@mui/material/styles";
import {
  OS3D_ICON_FILTER_VAR,
  type Os3dIconVariant,
} from "../theme/os3dLanguage";

function imgSxForVariant(
  variant: Os3dIconVariant,
  extra?: SxProps<Theme>,
): SxProps<Theme> {
  const base: SxProps<Theme> = {
    width: "100%",
    height: "100%",
    objectFit: "contain",
    filter: OS3D_ICON_FILTER_VAR[variant],
    transition:
      "filter 0.45s cubic-bezier(0.25, 0.46, 0.45, 0.94), transform 0.4s cubic-bezier(0.33, 1, 0.68, 1)",
    "@media (prefers-reduced-motion: reduce)": {
      transition: "none",
    },
  };
  return extra != null ? ([base, extra] as SxProps<Theme>) : base;
}

/**
 * 拟物 3D PNG 图标；滤镜档位见 `theme/os3dLanguage`（`Os3dIconVariant`），与壳层 OS3D token 一致。
 * 父级需给定尺寸（如 `icon-size-lg` 容器）。
 * 默认作装饰图：`aria-hidden`，`alt` 为空（若需读屏文案请设 `decorative={false}` 并给 `alt`）。
 */
export function Os3dIcon({
  src,
  alt = "",
  decorative = true,
  variant = "default",
  sx,
}: {
  src: string;
  alt?: string;
  /** 装饰性（无独立语义）：默认隐藏于读屏 */
  decorative?: boolean;
  /** 与场景匹配的投影档位：Dock / 磁贴 / Hero 等 */
  variant?: Os3dIconVariant;
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
      sx={imgSxForVariant(variant, sx)}
    />
  );
}

export type { Os3dIconVariant };

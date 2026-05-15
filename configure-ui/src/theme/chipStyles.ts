type TintedChipOptions = {
  height?: number;
  bgStrength?: number;
  borderStrength?: number;
  fontSize?: string;
  fontWeight?: number;
};

/**
 * 语义色 chip：文字、边框、底色必须来自同一个 accent，避免浅色底上混用灰蓝文字。
 */
export function createTintedChipSx(
  accentColor: string,
  {
    height = 24,
    bgStrength = 7,
    borderStrength = 18,
    fontSize = "var(--font-size-caption)",
    fontWeight = 600,
  }: TintedChipOptions = {},
) {
  return {
    height,
    borderRadius: "var(--radius-chip)",
    color: accentColor,
    bgcolor: `color-mix(in srgb, ${accentColor} ${bgStrength}%, var(--card))`,
    border: `1px solid color-mix(in srgb, ${accentColor} ${borderStrength}%, transparent)`,
    fontSize,
    fontWeight,
    "& .MuiChip-icon": {
      color: accentColor,
    },
  } as const;
}

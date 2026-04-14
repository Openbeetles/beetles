/**
 * Beetle OS 风格表单控件图标：浅井底 + 细描边 + 选中态主色（矢量，无整图 PNG）。
 * Vector form glyphs aligned with `--input-idle-well` / `--form-outline-rest` (no raster assets).
 * Avoid SVG <defs> ids so multiple instances on one page do not clash.
 */
import SvgIcon, { type SvgIconProps } from "@mui/material/SvgIcon";

const CHK = 4.25;
const CHK_SZ = 24 - 2 * CHK;

/** 未选：圆角井字 + 顶缘弱高光 */
export function OsCheckboxIcon(props: SvgIconProps) {
  return (
    <SvgIcon {...props} viewBox="0 0 24 24" fontSize="inherit">
      <rect
        x={CHK}
        y={CHK}
        width={CHK_SZ}
        height={CHK_SZ}
        rx={5.5}
        fill="var(--input-idle-well)"
        stroke="var(--outlined-border-rest)"
        strokeWidth={1.35}
      />
      <rect
        x={CHK + 1.25}
        y={CHK + 1.25}
        width={CHK_SZ - 2.5}
        height={4.75}
        rx={2}
        fill="var(--foreground)"
        opacity={0.06}
      />
    </SvgIcon>
  );
}

/** 已选：主色填充 + 顶高光条 + 对勾 */
export function OsCheckboxCheckedIcon(props: SvgIconProps) {
  return (
    <SvgIcon {...props} viewBox="0 0 24 24" fontSize="inherit">
      <rect
        x={CHK}
        y={CHK}
        width={CHK_SZ}
        height={CHK_SZ}
        rx={5.5}
        fill="var(--primary)"
        stroke="color-mix(in srgb, var(--primary) 72%, var(--foreground))"
        strokeWidth={1.2}
      />
      <rect
        x={CHK + 1}
        y={CHK + 1}
        width={CHK_SZ - 2}
        height={5.5}
        rx={2.25}
        fill="var(--primary-fg)"
        opacity={0.22}
      />
      <path
        fill="none"
        stroke="var(--primary-fg)"
        strokeWidth={2.35}
        strokeLinecap="round"
        strokeLinejoin="round"
        d="M8.2 12.1l2.6 2.6 5.4-5.8"
      />
    </SvgIcon>
  );
}

/** 半选：主色底 + 高对比短划 */
export function OsCheckboxIndeterminateIcon(props: SvgIconProps) {
  return (
    <SvgIcon {...props} viewBox="0 0 24 24" fontSize="inherit">
      <rect
        x={CHK}
        y={CHK}
        width={CHK_SZ}
        height={CHK_SZ}
        rx={5.5}
        fill="var(--primary)"
        stroke="color-mix(in srgb, var(--primary) 72%, var(--foreground))"
        strokeWidth={1.2}
      />
      <rect
        x={8}
        y={11}
        width={8}
        height={2.25}
        rx={1}
        fill="var(--primary-fg)"
      />
    </SvgIcon>
  );
}

const R_OUT = 9.25;
const R_DOT = 5.25;

export function OsRadioIcon(props: SvgIconProps) {
  return (
    <SvgIcon {...props} viewBox="0 0 24 24" fontSize="inherit">
      <circle
        cx={12}
        cy={12}
        r={R_OUT}
        fill="var(--input-idle-well)"
        stroke="var(--outlined-border-rest)"
        strokeWidth={1.35}
      />
      <ellipse
        cx={12}
        cy={9.75}
        rx={5.4}
        ry={3.1}
        fill="var(--foreground)"
        opacity={0.06}
      />
    </SvgIcon>
  );
}

export function OsRadioCheckedIcon(props: SvgIconProps) {
  return (
    <SvgIcon {...props} viewBox="0 0 24 24" fontSize="inherit">
      <circle
        cx={12}
        cy={12}
        r={R_OUT}
        fill="var(--input-idle-well)"
        stroke="var(--primary)"
        strokeWidth={1.9}
      />
      <circle cx={12} cy={12} r={R_DOT} fill="var(--primary)" />
      <circle
        cx={12}
        cy={10.4}
        r={R_DOT - 2.25}
        fill="var(--primary-fg)"
        opacity={0.35}
      />
    </SvgIcon>
  );
}

/**
 * 固件工具名（GET /api/tools 的 name）→ 3D PNG（见 `config/osIcons` `OS_ICON_TOOL`）。
 */
import { Os3dIcon } from "../components/Os3dIcon";
import { osIconSrcForToolName } from "../config/osIcons";

export function ToolGlyph({ name }: { name: string }) {
  return (
    <Os3dIcon
      src={osIconSrcForToolName(name)}
      variant="inline"
      sx={{
        width: "var(--icon-size-md)",
        height: "var(--icon-size-md)",
      }}
    />
  );
}

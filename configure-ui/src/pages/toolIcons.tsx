/**
 * 固件工具名（GET /api/tools 的 name）→ 3D PNG（见 `config/osIcons` `OS_ICON_TOOL`）。
 */
import { Os3dIcon } from "../components/Os3dIcon";
import { osIconSrcForToolName } from "../config/osIcons";
import { LAYOUT_TOKENS } from "../config/themeTokens";

export function ToolGlyph({ name }: { name: string }) {
  return (
    <Os3dIcon
      src={osIconSrcForToolName(name)}
      variant="inline"
      sx={{
        width: `${LAYOUT_TOKENS.toolsListIconPx}px`,
        height: `${LAYOUT_TOKENS.toolsListIconPx}px`,
      }}
    />
  );
}

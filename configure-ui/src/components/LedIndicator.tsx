import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";

/** 顶部状态条 LED，与 SystemStatusPanel 视觉语言一致。 */
export function LedIndicator({
  active,
  color,
  label,
}: {
  active: boolean;
  color: string;
  label: string;
}) {
  return (
    <Box sx={{ display: "flex", alignItems: "center", gap: 1, minWidth: 0 }}>
      <Box
        sx={{
          width: 8,
          height: 8,
          borderRadius: "50%",
          flexShrink: 0,
          bgcolor: active ? color : "var(--muted)",
          boxShadow: active ? `0 0 10px color-mix(in srgb, ${color} 55%, transparent)` : "none",
        }}
      />
      <Typography
        variant="caption"
        sx={{
          fontFamily: "var(--font-mono)",
          color: "var(--muted)",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {label}
      </Typography>
    </Box>
  );
}

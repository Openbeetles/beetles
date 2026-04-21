import Box from "@mui/material/Box";
import { InlineAlert } from "./InlineAlert";

export interface PageLoadErrorStateProps {
  message: string | null;
  onRetry?: () => void;
}

/**
 * 阻塞性加载失败：页面主体只保留一条玻璃告警，不再和 loading / empty / form 并排出现。
 */
export function PageLoadErrorState({
  message,
  onRetry,
}: PageLoadErrorStateProps) {
  if (!message) return null;
  return (
    <Box sx={{ width: "100%" }}>
      <InlineAlert message={message} onRetry={onRetry} />
    </Box>
  );
}

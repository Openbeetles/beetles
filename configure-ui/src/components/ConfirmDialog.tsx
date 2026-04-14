import type { ReactNode } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import Typography from "@mui/material/Typography";
import { useTranslation } from "react-i18next";
import { PANEL_SECTION_PADDING } from "../theme/panelStyles";

export interface ConfirmDialogProps {
  open: boolean;
  onClose: () => void;
  /** 标题 */
  title: string;
  /** 描述文案 */
  description: string;
  /** 标题前图标 */
  icon?: ReactNode;
  /** 确认按钮文案，默认 common 的 confirm 或“确认” */
  confirmLabel?: string;
  /** 取消按钮文案，默认 common.cancel */
  cancelLabel?: string;
  /** 点击取消时调用；若提供，则取消按钮不再调用 onClose（由本回调自行收尾） */
  onCancel?: () => void;
  /** 为 true 时禁止点击遮罩与 Esc 关闭，须点按钮 */
  requireExplicitAction?: boolean;
  /** 为 true 时使用较宽弹窗（如长说明） */
  wide?: boolean;
  /** 点击确认时调用；可异步，关闭由调用方在回调内处理或由 onClose 统一关闭 */
  onConfirm: () => void | Promise<void>;
  /** 确认按钮是否禁用（如提交中） */
  confirmDisabled?: boolean;
  /** 确认按钮 variant，默认 contained；危险操作可用 "contained" + confirmColor */
  confirmColor?: "primary" | "error" | "warning";
}

const ICON_COLOR: Record<
  NonNullable<ConfirmDialogProps["confirmColor"]>,
  string
> = {
  primary: "var(--primary)",
  error: "var(--semantic-danger)",
  warning: "var(--semantic-warning)",
};

/**
 * 通用操作确认弹窗：窄幅、左对齐、图标+标题一行，描述+按钮。
 */
export function ConfirmDialog({
  open,
  onClose,
  title,
  description,
  icon,
  confirmLabel,
  cancelLabel,
  onCancel,
  requireExplicitAction = false,
  wide = false,
  onConfirm,
  confirmDisabled = false,
  confirmColor = "primary",
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const handleConfirm = async () => {
    onClose();
    await onConfirm();
  };

  const handleDialogClose = (
    _: object,
    reason: "backdropClick" | "escapeKeyDown",
  ) => {
    if (
      requireExplicitAction &&
      (reason === "backdropClick" || reason === "escapeKeyDown")
    ) {
      return;
    }
    onClose();
  };

  const handleCancelClick = () => {
    if (onCancel) {
      onCancel();
    } else {
      onClose();
    }
  };

  return (
    <Dialog
      open={open}
      onClose={handleDialogClose}
      disableEscapeKeyDown={requireExplicitAction}
      maxWidth={wide ? "sm" : "xs"}
      fullWidth={wide}
      slotProps={{
        backdrop: { sx: { backgroundColor: "var(--backdrop-overlay)" } },
        paper: {
          sx: {
            width: "100%",
            maxWidth: wide ? undefined : "var(--dialog-narrow-max-width)",
            borderRadius: "var(--radius-card)",
            border: "none",
            boxShadow: "none",
            backgroundColor: "var(--card)",
          },
        },
      }}
      sx={{
        "& .MuiDialog-container": {
          alignItems: "center",
          justifyContent: "center",
        },
      }}
    >
      <Box sx={{ p: PANEL_SECTION_PADDING }}>
        <Box
          sx={{ display: "flex", alignItems: "flex-start", gap: 1.5, mb: 2 }}
        >
          {icon && (
            <Box
              sx={{
                width: "var(--icon-container-lg)",
                height: "var(--icon-container-lg)",
                borderRadius: "var(--radius-chip)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                flexShrink: 0,
                color: ICON_COLOR[confirmColor],
                backgroundColor:
                  "color-mix(in srgb, var(--foreground) 6%, transparent)",
              }}
            >
              <Box
                component="span"
                sx={{
                  display: "flex",
                  "& > svg": { fontSize: "var(--icon-size-md)" },
                }}
              >
                {icon}
              </Box>
            </Box>
          )}
          <Box sx={{ minWidth: 0, flex: 1 }}>
            <Typography
              sx={{
                fontSize: "var(--font-size-body)",
                fontWeight: 600,
                color: "var(--foreground)",
                lineHeight: "var(--line-height-snug)",
              }}
            >
              {title}
            </Typography>
            <Typography
              sx={{
                mt: 1.25,
                fontSize: "var(--font-size-body-sm)",
                color: "var(--muted)",
                lineHeight: wide
                  ? "var(--line-height-loose)"
                  : "var(--line-height-relaxed)",
                whiteSpace: "pre-line",
              }}
            >
              {description}
            </Typography>
          </Box>
        </Box>
        <Box sx={{ display: "flex", justifyContent: "center", gap: 1 }}>
          <Button
            variant="text"
            size="small"
            onClick={handleCancelClick}
            disabled={confirmDisabled}
            sx={{
              borderRadius: "var(--radius-control)",
              textTransform: "none",
              fontWeight: 600,
              color: "var(--muted)",
            }}
          >
            {cancelLabel ?? t("common.cancel", { defaultValue: "Cancel" })}
          </Button>
          <Button
            variant="contained"
            size="small"
            color={confirmColor}
            onClick={handleConfirm}
            disabled={confirmDisabled}
            sx={{
              borderRadius: "var(--radius-control)",
              textTransform: "none",
              fontWeight: 600,
              boxShadow: "none",
              "&:hover": { boxShadow: "none" },
            }}
          >
            {confirmLabel ?? t("common.confirm")}
          </Button>
        </Box>
      </Box>
    </Dialog>
  );
}

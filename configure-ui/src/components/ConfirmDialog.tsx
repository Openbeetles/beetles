import type { ReactNode } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import DialogActions from "@mui/material/DialogActions";
import DialogContent from "@mui/material/DialogContent";
import Typography from "@mui/material/Typography";
import { useTranslation } from "react-i18next";

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

const ICON_TINT: Record<
  NonNullable<ConfirmDialogProps["confirmColor"]>,
  string
> = {
  primary:
    "color-mix(in srgb, var(--primary) 14%, transparent)",
  error:
    "color-mix(in srgb, var(--semantic-danger) 16%, transparent)",
  warning:
    "color-mix(in srgb, var(--semantic-warning) 16%, transparent)",
};

const ICON_INK: Record<
  NonNullable<ConfirmDialogProps["confirmColor"]>,
  string
> = {
  primary: "var(--primary)",
  error: "var(--semantic-danger)",
  warning: "var(--semantic-warning)",
};

const TITLE_ID = "confirm-dialog-title";
const DESC_ID = "confirm-dialog-description";

/** 比面板略松一档，避免确认弹窗显得挤在一起 */
const DIALOG_PAD_X = 3;
const DIALOG_PAD_TOP = 3.5;
const DIALOG_PAD_BOTTOM = 3;
const ACTIONS_PAD_Y = 2.5;

/**
 * 通用操作确认弹窗：与壳层卡片一致的描边/轻投影，内容区与操作区分栏。
 * 有 `icon` 时图标置顶水平居中，标题与说明居中；无图标时文案左对齐。
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
      aria-labelledby={TITLE_ID}
      aria-describedby={DESC_ID}
      slotProps={{
        backdrop: {
          sx: {
            backgroundColor: "var(--backdrop-overlay)",
            backdropFilter: "blur(2px)",
          },
        },
        paper: {
          sx: {
            width: "100%",
            maxWidth: wide ? undefined : "var(--dialog-narrow-max-width)",
            borderRadius: "var(--radius-card)",
            border: "1px solid var(--form-outline-rest)",
            boxShadow:
              "0 8px 32px color-mix(in srgb, var(--foreground) 10%, transparent)",
            backgroundColor: "var(--card)",
            overflow: "hidden",
          },
        },
        transition: {
          timeout: { enter: 200, exit: 160 },
        },
      }}
      sx={{
        "& .MuiDialog-container": {
          alignItems: "center",
          justifyContent: "center",
        },
      }}
    >
      <DialogContent
        sx={{
          p: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <Box
          sx={{
            px: DIALOG_PAD_X,
            pt: DIALOG_PAD_TOP,
            pb: DIALOG_PAD_BOTTOM,
            display: "flex",
            flexDirection: "column",
            alignItems: icon ? "center" : "stretch",
            gap: icon ? 2.5 : 0,
          }}
        >
          {icon && (
            <Box
              sx={{
                width: 48,
                height: 48,
                borderRadius: "var(--radius-chip)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                flexShrink: 0,
                color: ICON_INK[confirmColor],
                backgroundColor: ICON_TINT[confirmColor],
                "& > span": {
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  maxWidth: "100%",
                  maxHeight: "100%",
                },
                "& > span > svg": {
                  fontSize: "var(--icon-size-lg) !important",
                },
              }}
            >
              <Box component="span" sx={{ lineHeight: 0 }}>
                {icon}
              </Box>
            </Box>
          )}
          <Box
            sx={{
              minWidth: 0,
              width: "100%",
              textAlign: icon ? "center" : "left",
            }}
          >
            <Typography
              id={TITLE_ID}
              component="h2"
              sx={{
                fontFamily: "var(--font-sans)",
                fontSize: "var(--font-size-body-lg)",
                fontWeight: 700,
                letterSpacing: "var(--letter-spacing-tight)",
                lineHeight: 1.45,
                color: "var(--foreground)",
              }}
            >
              {title}
            </Typography>
            <Typography
              id={DESC_ID}
              component="p"
              sx={{
                mt: 1.75,
                fontSize: "var(--font-size-body-sm)",
                color: "var(--text-tertiary)",
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
      </DialogContent>
      <DialogActions
        sx={{
          px: DIALOG_PAD_X,
          py: ACTIONS_PAD_Y,
          gap: 1.5,
          justifyContent: "flex-end",
          flexWrap: "wrap",
          borderTop: "1px solid var(--border-subtle)",
          backgroundColor:
            "color-mix(in srgb, var(--foreground) 2.5%, transparent)",
        }}
      >
        <Button
          variant="outlined"
          color="inherit"
          size="medium"
          onClick={handleCancelClick}
          disabled={confirmDisabled}
          sx={{
            borderRadius: "var(--radius-control)",
            textTransform: "none",
            fontWeight: 600,
            borderColor: "var(--border-subtle)",
            color: "var(--foreground-soft)",
            "&:hover": {
              borderColor: "var(--border)",
              backgroundColor:
                "color-mix(in srgb, var(--foreground) 5%, transparent)",
            },
          }}
        >
          {cancelLabel ?? t("common.cancel", { defaultValue: "Cancel" })}
        </Button>
        <Button
          variant="contained"
          size="medium"
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
      </DialogActions>
    </Dialog>
  );
}

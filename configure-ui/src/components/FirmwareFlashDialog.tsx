import { type ChangeEvent, useMemo, useState } from "react";
import UsbRounded from "@mui/icons-material/UsbRounded";
import CloseRounded from "@mui/icons-material/CloseRounded";
import CheckCircleRounded from "@mui/icons-material/CheckCircleRounded";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import Divider from "@mui/material/Divider";
import FormControl from "@mui/material/FormControl";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Typography from "@mui/material/Typography";
import type { SelectChangeEvent } from "@mui/material/Select";
import { useToast } from "../hooks/useToast";

type FlashBoard = {
  value: string;
  label: string;
  detail: string;
};

const FLASH_BOARD_OPTIONS: FlashBoard[] = [
  {
    value: "esp32-s3-8mb",
    label: "ESP32-S3 8MB",
    detail: "s3-8mb",
  },
  {
    value: "esp32-s3-16mb",
    label: "ESP32-S3 16MB",
    detail: "s3-16mb",
  },
  {
    value: "esp32-s3-32mb",
    label: "ESP32-S3 32MB",
    detail: "s3-32mb",
  },
  {
    value: "esp32-p4-nano-16mb",
    label: "ESP32-P4 Nano 16MB",
    detail: "p4-16mb",
  },
];

interface FirmwareFlashDialogProps {
  open: boolean;
  onClose: () => void;
}

export function FirmwareFlashDialog({ open, onClose }: FirmwareFlashDialogProps) {
  const { t } = useTranslation();
  const { showToast } = useToast();
  const [board, setBoard] = useState<string>(FLASH_BOARD_OPTIONS[0]?.value ?? "");
  const [firmwareName, setFirmwareName] = useState<string>("");
  const [running, setRunning] = useState(false);
  const closeModal = () => {
    if (!running) {
      onClose();
    }
  };

  const supportsUsb = useMemo(() => {
    return typeof window !== "undefined" && !!(window.navigator as unknown as { serial?: unknown })?.serial;
  }, []);

  const boardLabel = useMemo(
    () => FLASH_BOARD_OPTIONS.find((item) => item.value === board)?.label ?? "",
    [board],
  );

  const handleBoardChange = (event: SelectChangeEvent<string>) => {
    setBoard(event.target.value);
  };

  const handleFileInput = (event: ChangeEvent<HTMLInputElement>) => {
    const nextFile = event.currentTarget.files?.[0];
    setFirmwareName(nextFile?.name ?? "");
  };

  const handleSubmit = async () => {
    if (!supportsUsb) {
      showToast(t("device.flashSerialUnsupported"), {
        variant: "error",
      });
      return;
    }
    if (!firmwareName) {
      showToast(t("device.flashNeedFile"), {
        variant: "warning",
      });
      return;
    }

    setRunning(true);
    try {
      await new Promise<void>((resolve) => setTimeout(resolve, 600));
      showToast(t("device.flashNotImplemented"), {
        variant: "warning",
      });
    } finally {
      setRunning(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={closeModal}
      maxWidth="sm"
      fullWidth
      aria-labelledby="firmware-flash-dialog-title"
      slotProps={{
        paper: {
          sx: {
            borderRadius: "var(--radius-card)",
            border: "1px solid var(--form-outline-rest)",
            backgroundColor: "var(--card)",
            boxShadow: "var(--os3d-content-plate-stack)",
            transformOrigin: "top right",
          },
        },
        transition: {
          timeout: { enter: 220, exit: 140 },
        },
      }}
      sx={{
        "& .MuiDialog-paper": {
          width: "min(780px, calc(100vw - 24px))",
        },
      }}
    >
      <Box
        sx={{
          position: "relative",
          overflow: "hidden",
        }}
      >
        <DialogTitle
          id="firmware-flash-dialog-title"
          sx={{
            pr: 6,
            fontFamily: "var(--font-brand)",
            fontWeight: 700,
            letterSpacing: "-0.02em",
            display: "flex",
            alignItems: "center",
            gap: 1.5,
          }}
        >
          <UsbRounded
            sx={{
              width: 22,
              height: 22,
              color: "var(--primary)",
            }}
          />
          {t("device.flashModalTitle")}
          <Button
            onClick={closeModal}
            size="small"
            variant="text"
            aria-label={t("device.flashModalClose")}
            disabled={running}
            sx={{
              position: "absolute",
              right: 8,
              top: 10,
              minWidth: 36,
              height: 36,
              borderRadius: "50%",
              color: "var(--foreground-soft)",
            }}
          >
            <CloseRounded />
          </Button>
        </DialogTitle>
        <DialogContent
          sx={{
            pt: 0,
            pb: 2,
          }}
        >
          <Typography
            variant="body2"
            sx={{
              color: "var(--foreground-soft)",
              mb: 2,
              whiteSpace: "pre-line",
            }}
          >
            {t("device.flashModalDesc")}
          </Typography>

          <Box
            sx={{
              display: "grid",
              gap: 2,
            }}
          >
            <Box
              sx={{
                display: "grid",
                gap: 0.75,
                p: 2,
                borderRadius: "var(--radius-chip)",
                border: "1px solid color-mix(in srgb, var(--border) 36%, transparent)",
                backgroundColor: "color-mix(in srgb, var(--surface) 72%, transparent)",
              }}
            >
              <Typography
                variant="body2"
                sx={{
                  fontWeight: 700,
                  color: "var(--foreground-soft)",
                }}
              >
                {t("device.flashStepBoard")}
              </Typography>
              <FormControl fullWidth size="small">
                <InputLabel id="firmware-board-label">
                  {t("device.flashBoardLabel")}
                </InputLabel>
                <Select
                  labelId="firmware-board-label"
                  value={board}
                  label={t("device.flashBoardLabel")}
                  onChange={handleBoardChange}
                >
                  {FLASH_BOARD_OPTIONS.map((item) => (
                    <MenuItem key={item.value} value={item.value}>
                      <Box>
                        <Typography component="div" variant="body2">
                          {item.label}
                        </Typography>
                        <Typography component="div" variant="caption" color="text.secondary">
                          {item.detail}
                        </Typography>
                      </Box>
                    </MenuItem>
                  ))}
                </Select>
              </FormControl>
              <Typography variant="caption" sx={{ color: "var(--foreground-soft)" }}>
                {t("device.flashSelectedBoard")}：{boardLabel}
              </Typography>
            </Box>

            <Box
              sx={{
                display: "grid",
                gap: 0.75,
                p: 2,
                borderRadius: "var(--radius-chip)",
                border: "1px solid color-mix(in srgb, var(--border) 36%, transparent)",
                backgroundColor: "color-mix(in srgb, var(--surface) 72%, transparent)",
              }}
            >
              <Typography
                variant="body2"
                sx={{
                  fontWeight: 700,
                  color: "var(--foreground-soft)",
                }}
              >
                {t("device.flashStepFirmware")}
              </Typography>
              <Button
                component="label"
                variant="outlined"
                fullWidth
                sx={{
                  justifyContent: "flex-start",
                  borderRadius: "var(--radius-chip)",
                }}
              >
                {firmwareName ? firmwareName : t("device.flashChooseFirmware")}
                <input
                  type="file"
                  accept=".bin,.zip"
                  hidden
                  onChange={handleFileInput}
                />
              </Button>
              {!firmwareName ? (
                <Typography variant="caption" sx={{ color: "var(--foreground-soft)" }}>
                  {t("device.flashNeedFile")}
                </Typography>
              ) : null}
            </Box>

            <Box
              sx={{
                display: "grid",
                gap: 0.75,
                p: 2,
                borderRadius: "var(--radius-chip)",
                border: "1px solid color-mix(in srgb, var(--border) 36%, transparent)",
                backgroundColor: supportsUsb
                  ? "color-mix(in srgb, var(--semantic-success) 14%, transparent)"
                  : "color-mix(in srgb, var(--semantic-danger) 14%, transparent)",
              }}
            >
              <Typography
                variant="body2"
                sx={{
                  fontWeight: 700,
                  color: supportsUsb ? "var(--semantic-success)" : "var(--semantic-danger)",
                  display: "flex",
                  alignItems: "center",
                  gap: 0.75,
                }}
              >
                <CheckCircleRounded sx={{ width: 16, height: 16 }} />
                {supportsUsb
                  ? t("device.flashSerialSupported")
                  : t("device.flashSerialUnsupported")}
              </Typography>
              {!supportsUsb ? (
                <Typography variant="caption" sx={{ color: "var(--foreground-soft)" }}>
                  {t("device.flashSerialHint")}
                </Typography>
              ) : null}
            </Box>
          </Box>

          <Divider sx={{ mt: 2.5, mb: 2.5 }} />
          <Typography
            variant="caption"
            sx={{
              color: "var(--foreground-soft)",
            }}
          >
            {t("device.flashModalTips")}
          </Typography>

          <Box
            sx={{
              mt: 1.5,
              display: "flex",
              justifyContent: "flex-end",
              gap: 1,
            }}
          >
            <Button variant="text" onClick={closeModal} disabled={running}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="contained"
              onClick={handleSubmit}
              disabled={running || !firmwareName}
            >
              {running ? t("device.flashRunning") : t("device.flashStart")}
            </Button>
          </Box>
        </DialogContent>
      </Box>
    </Dialog>
  );
}

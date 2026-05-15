import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import Box from "@mui/material/Box";
import { ConfigSubNavLayout } from "../components/ConfigSubNavLayout";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { PAGE_COLUMN_FILL_SX } from "../theme/panelStyles";
import { OS_ICON_DEVICE_CONFIG } from "../config/osIcons";

/**
 * 设备配置壳层：左侧分区导航 + 右侧子路由（`/device-config/:tab`）。
 * /device-config/display — 显示；/device-config/audio — 音频；
 * /device-config/hardware — GPIO 等硬件设备；/device-config/i2c-sensors — I2C 传感器
 */
export function DeviceConfigLayout() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const prevPathRef = useRef<string | null>(null);
  const [disclaimerOpen, setDisclaimerOpen] = useState(false);

  useEffect(() => {
    const enteredFromOutside =
      prevPathRef.current === null ||
      !prevPathRef.current.startsWith("/device-config");
    if (pathname.startsWith("/device-config") && enteredFromOutside) {
      /**
       * 必须同步打开：若用 `queueMicrotask` + cleanup `cancelled`，在 React 18
       * `StrictMode` 下会先卸载再挂载，微任务被跳过，弹窗永远不出现。
       */
      setDisclaimerOpen(true);
    }
    prevPathRef.current = pathname;
  }, [pathname]);

  const subNavItems = [
    {
      segment: "display",
      label: t("deviceConfig.tabDisplay"),
      iconSrc: OS_ICON_DEVICE_CONFIG.display,
    },
    {
      segment: "audio",
      label: t("deviceConfig.tabAudio"),
      iconSrc: OS_ICON_DEVICE_CONFIG.audio,
    },
    {
      segment: "hardware",
      label: t("deviceConfig.tabGpioDevices"),
      iconSrc: OS_ICON_DEVICE_CONFIG.hardware,
    },
    {
      segment: "i2c-sensors",
      label: t("deviceConfig.tabI2cSensors"),
      iconSrc: OS_ICON_DEVICE_CONFIG.i2cSensors,
    },
  ];

  return (
    <Box sx={PAGE_COLUMN_FILL_SX}>
      <ConfirmDialog
        open={disclaimerOpen}
        onClose={() => setDisclaimerOpen(false)}
        onCancel={() => {
          navigate("/device");
          setDisclaimerOpen(false);
        }}
        requireExplicitAction
        wide
        title={t("deviceConfig.disclaimerTitle")}
        description={t("deviceConfig.disclaimerDesc")}
        dialogIcon="warning"
        confirmColor="error"
        cancelLabel={t("deviceConfig.disclaimerLeave")}
        confirmLabel={t("deviceConfig.disclaimerContinue")}
        onConfirm={() => {}}
      />
      <ConfigSubNavLayout basePath="/device-config" items={subNavItems} />
    </Box>
  );
}

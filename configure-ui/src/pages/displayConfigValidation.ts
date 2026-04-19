import type { DisplayConfig } from "../types/displayConfig.ts";
import type { DeviceRuntimeKind } from "../store/deviceStatusStore.ts";

const PIN_MIN = 1;
const PIN_MAX = 48;
const DIM_MIN = 1;
const DIM_MAX = 480;
const OFFSET_MIN = -480;
const OFFSET_MAX = 480;
const FREQ_MIN = 1_000_000;
const FREQ_MAX = 80_000_000;

function pathNoControlChars(value: string): boolean {
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code < 0x20 || code === 0) return false;
  }
  return true;
}

function validateEspDisplayConfig(
  form: DisplayConfig,
  t: (key: string) => string,
): string | null {
  if (!form.enabled) return null;
  const dimOk =
    form.width >= DIM_MIN &&
    form.width <= DIM_MAX &&
    form.height >= DIM_MIN &&
    form.height <= DIM_MAX;
  if (!dimOk) return t("displayConfig.validation.dimension");
  if (![0, 90, 180, 270].includes(form.rotation)) {
    return t("displayConfig.validation.rotation");
  }
  const offsetOk =
    form.offset_x >= OFFSET_MIN &&
    form.offset_x <= OFFSET_MAX &&
    form.offset_y >= OFFSET_MIN &&
    form.offset_y <= OFFSET_MAX;
  if (!offsetOk) return t("displayConfig.validation.offset");
  if (form.spi.freq_hz < FREQ_MIN || form.spi.freq_hz > FREQ_MAX) {
    return t("displayConfig.validation.freq");
  }
  const pins = [form.spi.sclk, form.spi.mosi, form.spi.cs, form.spi.dc];
  if (pins.some((pin) => pin < PIN_MIN || pin > PIN_MAX)) {
    return t("displayConfig.validation.pin");
  }
  if (
    form.spi.rst != null &&
    (form.spi.rst < PIN_MIN || form.spi.rst > PIN_MAX)
  ) {
    return t("displayConfig.validation.pin");
  }
  if (form.spi.bl != null && (form.spi.bl < PIN_MIN || form.spi.bl > PIN_MAX)) {
    return t("displayConfig.validation.pin");
  }
  return null;
}

function validateLinuxFramebufferConfig(
  form: DisplayConfig,
  t: (key: string) => string,
): string | null {
  if (!form.enabled) return null;
  const dimOk =
    form.width >= DIM_MIN &&
    form.width <= DIM_MAX &&
    form.height >= DIM_MIN &&
    form.height <= DIM_MAX;
  if (!dimOk) return t("displayConfig.validation.dimension");
  const framebufferPath = form.fb_device.trim();
  if (!framebufferPath) return t("displayConfig.validation.fbDeviceRequired");
  if (!pathNoControlChars(framebufferPath)) {
    return t("displayConfig.validation.pathInvalid");
  }
  const backlightPath = form.backlight_sysfs?.trim() ?? "";
  if (backlightPath && !pathNoControlChars(backlightPath)) {
    return t("displayConfig.validation.pathInvalid");
  }
  return null;
}

function validateLinuxSpiConfig(
  form: DisplayConfig,
  t: (key: string) => string,
): string | null {
  if (!form.enabled) return null;
  const dimOk =
    form.width >= DIM_MIN &&
    form.width <= DIM_MAX &&
    form.height >= DIM_MIN &&
    form.height <= DIM_MAX;
  if (!dimOk) return t("displayConfig.validation.dimension");
  if (![0, 90, 180, 270].includes(form.rotation)) {
    return t("displayConfig.validation.rotation");
  }
  const offsetOk =
    form.offset_x >= OFFSET_MIN &&
    form.offset_x <= OFFSET_MAX &&
    form.offset_y >= OFFSET_MIN &&
    form.offset_y <= OFFSET_MAX;
  if (!offsetOk) return t("displayConfig.validation.offset");
  if (form.spi.freq_hz < FREQ_MIN || form.spi.freq_hz > FREQ_MAX) {
    return t("displayConfig.validation.freq");
  }
  const gpioPins = [form.spi.dc, form.spi.rst, form.spi.bl].filter(
    (pin): pin is number => pin != null,
  );
  if (gpioPins.some((pin) => pin < PIN_MIN || pin > PIN_MAX)) {
    return t("displayConfig.validation.pin");
  }
  const devicePath = form.fb_device.trim();
  if (devicePath && !pathNoControlChars(devicePath)) {
    return t("displayConfig.validation.pathInvalid");
  }
  return null;
}

export function validateDisplayConfigForRuntime(
  form: DisplayConfig,
  runtimeKind: DeviceRuntimeKind,
  t: (key: string) => string,
): string | null {
  if (runtimeKind === "linux") {
    if (form.driver === "framebuffer") {
      return validateLinuxFramebufferConfig(form, t);
    }
    return validateLinuxSpiConfig(form, t);
  }
  return validateEspDisplayConfig(form, t);
}

export function linuxBusForDriver(
  driver: DisplayConfig["driver"],
): DisplayConfig["bus"] {
  return driver === "framebuffer" ? "framebuffer" : "spi";
}

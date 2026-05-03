export type FlashBoard = {
  value: string;
  label: string;
  chipName: string;
  flashSize: string;
};

export type FlashDeviceInfo = {
  chipName: string;
  chipDescription: string;
  flashSize: string;
  psramSize: string | null;
  features: string[];
};

export type FirmwareFlashCloseReason =
  | "explicit"
  | "backdropClick"
  | "escapeKeyDown";

export type FirmwareFlashMode = "update" | "reinstall";

export type FirmwareFlashFailurePhase = "selectDevice" | "submitFlash";

export type FirmwareFlashPartPlan = {
  address: number;
  sizeBytes: number;
};

export type FirmwareFlashPreservedRange = {
  start: number;
  end: number;
};

export const FLASH_BOARD_OPTIONS: FlashBoard[] = [
  {
    value: "esp32-s3-8mb",
    label: "ESP32-S3 8MB",
    chipName: "ESP32-S3",
    flashSize: "8MB",
  },
  {
    value: "esp32-s3-16mb",
    label: "ESP32-S3 16MB",
    chipName: "ESP32-S3",
    flashSize: "16MB",
  },
  {
    value: "esp32-s3-32mb",
    label: "ESP32-S3 32MB",
    chipName: "ESP32-S3",
    flashSize: "32MB",
  },
  {
    value: "esp32-p4-nano-16mb",
    label: "ESP32-P4 Nano 16MB",
    chipName: "ESP32-P4",
    flashSize: "16MB",
  },
];

export function normalizeMemorySize(size: string): string {
  return size.replace(/\s+/g, "").toUpperCase();
}

export function formatMemorySize(size: string): string {
  return normalizeMemorySize(size).replace(/(\d+(?:\.\d+)?)(KB|MB)$/u, "$1 $2");
}

export function parseMemorySizeBytes(size: string): number | null {
  const match = normalizeMemorySize(size).match(/^(\d+(?:\.\d+)?)(KB|MB)$/u);
  if (match?.[1] == null || match[2] == null) return null;
  const value = Number(match[1]);
  if (!Number.isFinite(value) || value <= 0) return null;
  return Math.floor(value * (match[2] === "MB" ? 1024 * 1024 : 1024));
}

export function extractMemoryFeature(features: string[], kind: "Flash" | "PSRAM") {
  const matcher = new RegExp(`${kind}\\s+(\\d+(?:\\.\\d+)?)\\s*(KB|MB)`, "iu");
  for (const feature of features) {
    const match = feature.match(matcher);
    if (match?.[1] != null && match[2] != null) {
      return `${match[1]}${match[2].toUpperCase()}`;
    }
  }
  return null;
}

export function formatFlashDeviceLabel(info: FlashDeviceInfo): string {
  const parts = [
    info.chipDescription,
    `Flash ${formatMemorySize(info.flashSize)}`,
  ];
  if (info.psramSize != null) {
    parts.push(`PSRAM ${formatMemorySize(info.psramSize)}`);
  }
  return parts.join(" · ");
}

export function resolveFlashBoard(info: FlashDeviceInfo): FlashBoard | null {
  return (
    FLASH_BOARD_OPTIONS.find(
      (board) =>
        board.chipName === info.chipName &&
        normalizeMemorySize(board.flashSize) === normalizeMemorySize(info.flashSize),
    ) ?? null
  );
}

export function shouldCloseFirmwareFlashDialog(
  reason: FirmwareFlashCloseReason,
): boolean {
  return reason === "explicit";
}

export function shouldEraseBeforeFirmwareFlash(
  mode: FirmwareFlashMode,
): boolean {
  return mode === "reinstall";
}

export function nextFlashDeviceSelectionAfterFailure(
  currentIndex: string,
  phase: FirmwareFlashFailurePhase,
): string {
  return phase === "submitFlash" ? currentIndex : "";
}

export function flashPartsTouchPreservedRanges(
  parts: FirmwareFlashPartPlan[],
  ranges: FirmwareFlashPreservedRange[],
): boolean {
  return parts.some((part) => {
    if (part.sizeBytes <= 0) return false;
    const partEnd = part.address + part.sizeBytes;
    return ranges.some((range) => part.address < range.end && partEnd > range.start);
  });
}

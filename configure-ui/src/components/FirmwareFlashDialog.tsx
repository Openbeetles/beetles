import { type MouseEvent, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ESPLoader,
  Transport,
  type FlashOptions,
  type IEspLoaderTerminal,
} from "esptool-js";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import CircularProgress from "@mui/material/CircularProgress";
import Dialog from "@mui/material/Dialog";
import DialogContent from "@mui/material/DialogContent";
import DialogTitle from "@mui/material/DialogTitle";
import FormControl from "@mui/material/FormControl";
import FormHelperText from "@mui/material/FormHelperText";
import InputLabel from "@mui/material/InputLabel";
import MenuItem from "@mui/material/MenuItem";
import Select, { type SelectChangeEvent } from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import ToggleButton from "@mui/material/ToggleButton";
import ToggleButtonGroup from "@mui/material/ToggleButtonGroup";
import Typography from "@mui/material/Typography";
import { useToast } from "../hooks/useToast";
import {
  DIALOG_FORM_SCROLL_WELL_SX,
  DIALOG_FORM_SUBMIT_BAR_SX,
} from "../theme/panelStyles";
import { FormGrid } from "./form";
import {
  FLASH_BOARD_OPTIONS,
  extractMemoryFeature,
  flashPartsTouchPreservedRanges,
  firmwareFlashRequiredUpdatePartKinds,
  formatFlashDeviceLabel,
  formatMemorySize,
  isFirmwareFlashUpdatePartKind,
  nextFlashDeviceSelectionAfterFailure,
  normalizeMemorySize,
  parseMemorySizeBytes,
  resolveFlashBoard,
  shouldCloseFirmwareFlashDialog,
  shouldEraseBeforeFirmwareFlash,
  type FlashBoard,
  type FirmwareFlashCloseReason,
  type FirmwareFlashMode,
  type FirmwareFlashUpdatePartKind,
  type FlashDeviceInfo,
} from "./firmwareFlashModel";

type FlashDeviceEntry = {
  port: SerialPortLike;
  info: FlashDeviceInfo;
};

type FirmwareCatalogEntry = {
  boardId: string;
  bin: FirmwareCatalogAsset;
  flashSize: string | null;
  updateParts: FirmwareCatalogUpdatePart[];
};

type FirmwareCatalogAsset = {
  file: string;
  sha256: string;
  sizeBytes: number | null;
};

type FirmwareCatalogUpdatePart = FirmwareCatalogAsset & {
  kind: FirmwareFlashUpdatePartKind;
  offset: number;
};

type FirmwareFlashFile = {
  address: number;
  data: Uint8Array;
  fileName: string;
  kind: "merged" | FirmwareCatalogUpdatePart["kind"];
};

type FirmwareFlashBundle = {
  boardId: string;
  files: FirmwareFlashFile[];
  mode: FirmwareFlashMode;
};

type SerialPortInfo = {
  usbVendorId?: number;
  usbProductId?: number;
};

type SerialPortFilter = {
  usbVendorId: number;
};

type SerialRequestOptions = {
  filters?: SerialPortFilter[];
};

type SerialPortLike = {
  getInfo?: () => SerialPortInfo;
};

type SerialApiLike = {
  getPorts: () => Promise<SerialPortLike[]>;
  requestPort: (options?: SerialRequestOptions) => Promise<SerialPortLike>;
};

const FLASH_DIALOG_TITLE_ID = "firmware-flash-dialog-title";
const FLASH_DEVICE_BAUD_RATE = 115_200;
const FLASH_FIRMWARE_ADDRESS = 0x0;
const DEFAULT_FIRMWARE_BUNDLE_BASE_URL = "/firmware";
const ESP_IMAGE_MAGIC = 0xe9;
const ESP_PARTITION_TABLE_OFFSET = 0x8000;
const ESP_APP_IMAGE_OFFSET = 0x20000;
const ESP_NVS_OFFSET = 0x9000;
const ESP_NVS_END = 0x19000;
const SUPPORTED_FLASH_VENDOR_IDS = new Set([
  0x303a, // Espressif native USB
  0x10c4, // Silicon Labs CP210x
  0x1a86, // WCH CH34x
  0x0403, // FTDI
  0x067b, // Prolific PL2303
]);
const FLASH_DEVICE_FILTERS = Array.from(SUPPORTED_FLASH_VENDOR_IDS).map(
  (usbVendorId) => ({ usbVendorId }),
);
const FLASH_LOADER_TERMINAL: IEspLoaderTerminal = {
  clean: () => undefined,
  write: () => undefined,
  writeLine: () => undefined,
};

class FlashDeviceError extends Error {
  readonly translationKey: string;
  readonly params?: Record<string, string>;

  constructor(translationKey: string, params?: Record<string, string>) {
    super(translationKey);
    this.translationKey = translationKey;
    this.params = params;
  }
}

function getSerialApi(): SerialApiLike | null {
  if (typeof window === "undefined") return null;
  return (
    (window.navigator as unknown as { serial?: SerialApiLike }).serial ?? null
  );
}

function getFirmwareBundleBaseUrl(): string {
  const configured = import.meta.env.VITE_ESP_FIRMWARE_BASE_URL;
  return (configured?.trim() || DEFAULT_FIRMWARE_BUNDLE_BASE_URL).replace(
    /\/+$/u,
    "",
  );
}

function firmwareAssetUrl(baseUrl: string, fileName: string): string {
  const origin =
    typeof window === "undefined" ? "http://localhost" : window.location.origin;
  const base = new URL(`${baseUrl}/`, origin);
  return new URL(fileName, base).toString();
}

function normalizeSha256(value: string): string | null {
  const normalized = value.trim().toLowerCase();
  return /^[a-f0-9]{64}$/u.test(normalized) ? normalized : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value != null && !Array.isArray(value);
}

function readStringField(
  value: Record<string, unknown>,
  key: string,
): string | null {
  const field = value[key];
  return typeof field === "string" && field.trim() !== ""
    ? field.trim()
    : null;
}

function readNumberField(
  value: Record<string, unknown>,
  key: string,
): number | null {
  const field = value[key];
  return typeof field === "number" && Number.isFinite(field) && field >= 0
    ? field
    : null;
}

function flashDeviceErrorMessage(
  error: unknown,
  t: (key: string, options?: Record<string, string>) => string,
): string {
  if (error instanceof FlashDeviceError) {
    return t(error.translationKey, error.params);
  }
  return t("device.flashDeviceConnectFailed");
}

function ensureSupportedUsbBridge(port: SerialPortLike) {
  const info = port.getInfo?.();
  if (
    info?.usbVendorId != null &&
    !SUPPORTED_FLASH_VENDOR_IDS.has(info.usbVendorId)
  ) {
    throw new FlashDeviceError("device.flashDeviceUnsupported");
  }
}

function ensureSupportedFlashBoard(info: FlashDeviceInfo) {
  const board = resolveFlashBoard(info);
  if (board == null) {
    throw new FlashDeviceError("device.flashDeviceUnsupportedBoard", {
      actual: formatFlashDeviceLabel(info),
    });
  }
  return board;
}

function inferFirmwareBoardFromName(fileName: string): FlashBoard | null {
  const normalized = fileName.trim().toLowerCase();
  if (!normalized.endsWith(".bin")) return null;
  return (
    FLASH_BOARD_OPTIONS.find((board) =>
      normalized.endsWith(`${board.value}.bin`),
    ) ?? null
  );
}

function hasPartitionTableMagic(
  data: Uint8Array,
  offset = ESP_PARTITION_TABLE_OFFSET,
): boolean {
  const first = data[offset];
  const second = data[offset + 1];
  return (
    (first === 0xaa && second === 0x50) ||
    (first === 0x50 && second === 0xaa)
  );
}

function validateMergedFirmwareImage(
  data: Uint8Array,
  deviceInfo: FlashDeviceInfo,
) {
  const flashBytes = parseMemorySizeBytes(deviceInfo.flashSize);
  if (flashBytes != null && data.byteLength > flashBytes) {
    throw new FlashDeviceError("device.flashFirmwareTooLarge", {
      actual: `${Math.ceil(data.byteLength / (1024 * 1024))} MB`,
      limit: formatMemorySize(deviceInfo.flashSize),
    });
  }
  if (
    data.byteLength <= ESP_APP_IMAGE_OFFSET ||
    data[0] !== ESP_IMAGE_MAGIC ||
    data[ESP_APP_IMAGE_OFFSET] !== ESP_IMAGE_MAGIC ||
    !hasPartitionTableMagic(data)
  ) {
    throw new FlashDeviceError("device.flashFirmwareInvalid");
  }
}

function validateFirmwareFileForDevice(
  fileName: string,
  data: Uint8Array,
  deviceInfo: FlashDeviceInfo,
) {
  const deviceBoard = ensureSupportedFlashBoard(deviceInfo);
  const firmwareBoard = inferFirmwareBoardFromName(fileName);
  if (firmwareBoard == null) {
    throw new FlashDeviceError("device.flashFirmwareInvalid");
  }
  if (firmwareBoard.value !== deviceBoard.value) {
    throw new FlashDeviceError("device.flashFirmwareBoardMismatch", {
      expected: deviceBoard.label,
      actual: firmwareBoard.label,
    });
  }
  validateMergedFirmwareImage(data, deviceInfo);
}

function readCatalogAsset(value: unknown): FirmwareCatalogAsset | null {
  if (!isRecord(value)) return null;
  const file = readStringField(value, "file");
  const rawSha256 = readStringField(value, "sha256");
  const sha256 = rawSha256 == null ? null : normalizeSha256(rawSha256);
  if (file == null || sha256 == null) return null;
  return {
    file,
    sha256,
    sizeBytes: readNumberField(value, "size_bytes"),
  };
}

function readCatalogUpdateParts(value: unknown): FirmwareCatalogUpdatePart[] {
  if (!Array.isArray(value)) return [];
  const parts: FirmwareCatalogUpdatePart[] = [];
  for (const item of value) {
    const asset = readCatalogAsset(item);
    if (asset == null || !isRecord(item)) continue;
    const kind = readStringField(item, "kind");
    const offset = readNumberField(item, "offset");
    if (offset == null || !isFirmwareFlashUpdatePartKind(kind)) {
      continue;
    }
    parts.push({ ...asset, kind, offset });
  }
  return parts;
}

function parseFirmwareCatalog(text: string): FirmwareCatalogEntry[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  if (!isRecord(parsed) || parsed.product !== "beetle") {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  const boards = parsed.boards;
  if (!Array.isArray(boards)) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  const entries: FirmwareCatalogEntry[] = [];
  for (const board of boards) {
    if (!isRecord(board)) continue;
    const boardId = readStringField(board, "id");
    const flashSize = readStringField(board, "flash_size");
    const bin = readCatalogAsset(board.bin);
    if (boardId == null || bin == null) continue;
    entries.push({
      boardId,
      bin,
      flashSize,
      updateParts: readCatalogUpdateParts(board.update_parts),
    });
  }
  if (entries.length === 0) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  return entries;
}

async function sha256Hex(data: Uint8Array): Promise<string> {
  const subtle = globalThis.crypto?.subtle;
  if (subtle == null) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  const digestInput = new Uint8Array(data);
  const digest = await subtle.digest("SHA-256", digestInput.buffer);
  return Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

async function validateFirmwareCatalogAsset({
  asset,
  data,
}: {
  asset: FirmwareCatalogAsset;
  data: Uint8Array;
}) {
  if (asset.sizeBytes != null && asset.sizeBytes !== data.byteLength) {
    throw new FlashDeviceError("device.flashFirmwareChecksumMismatch");
  }
  const actualSha256 = await sha256Hex(data);
  if (actualSha256 !== asset.sha256) {
    throw new FlashDeviceError("device.flashFirmwareChecksumMismatch");
  }
}

async function fetchFirmwareAsset(
  bundleBaseUrl: string,
  asset: FirmwareCatalogAsset,
): Promise<Uint8Array> {
  let data: Uint8Array;
  try {
    const firmwareResponse = await fetch(
      firmwareAssetUrl(bundleBaseUrl, asset.file),
      { cache: "no-store" },
    );
    if (!firmwareResponse.ok) {
      throw new Error(`firmware ${firmwareResponse.status}`);
    }
    data = new Uint8Array(await firmwareResponse.arrayBuffer());
  } catch {
    throw new FlashDeviceError("device.flashFirmwareSourceUnavailable");
  }
  await validateFirmwareCatalogAsset({ asset, data });
  return data;
}

function ensureCatalogEntryMatchesBoard(
  entry: FirmwareCatalogEntry,
  firmwareBoard: FlashBoard,
) {
  if (
    entry.flashSize != null &&
    normalizeMemorySize(entry.flashSize) !==
      normalizeMemorySize(firmwareBoard.flashSize)
  ) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
}

function validateFirmwareUpdateFiles(
  files: FirmwareFlashFile[],
  deviceInfo: FlashDeviceInfo,
) {
  const firmwareBoard = ensureSupportedFlashBoard(deviceInfo);
  const requiredKinds = new Set(
    firmwareFlashRequiredUpdatePartKinds(firmwareBoard),
  );
  for (const file of files) {
    if (file.kind !== "merged") {
      requiredKinds.delete(file.kind);
    }
  }
  if (requiredKinds.size > 0) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  const flashBytes = parseMemorySizeBytes(deviceInfo.flashSize);
  for (const file of files) {
    if (flashBytes != null && file.address + file.data.byteLength > flashBytes) {
      throw new FlashDeviceError("device.flashFirmwareTooLarge", {
        actual: `${Math.ceil((file.address + file.data.byteLength) / (1024 * 1024))} MB`,
        limit: formatMemorySize(deviceInfo.flashSize),
      });
    }
  }
  if (
    flashPartsTouchPreservedRanges(
      files.map((file) => ({
        address: file.address,
        sizeBytes: file.data.byteLength,
      })),
      [{ start: ESP_NVS_OFFSET, end: ESP_NVS_END }],
    )
  ) {
    throw new FlashDeviceError("device.flashFirmwareInvalid");
  }
  const bootloader = files.find((file) => file.kind === "bootloader");
  const partitionTable = files.find((file) => file.kind === "partition-table");
  const app = files.find((file) => file.kind === "app");
  if (
    bootloader?.data[0] !== ESP_IMAGE_MAGIC ||
    partitionTable == null ||
    !hasPartitionTableMagic(partitionTable.data, 0) ||
    app?.data[0] !== ESP_IMAGE_MAGIC
  ) {
    throw new FlashDeviceError("device.flashFirmwareInvalid");
  }
}

function validateFirmwareBundleForDevice(
  bundle: FirmwareFlashBundle,
  deviceInfo: FlashDeviceInfo,
) {
  const deviceBoard = ensureSupportedFlashBoard(deviceInfo);
  if (bundle.boardId !== deviceBoard.value) {
    throw new FlashDeviceError("device.flashFirmwareBoardMismatch", {
      expected: deviceBoard.label,
      actual: bundle.boardId,
    });
  }
  if (bundle.mode === "reinstall") {
    const merged = bundle.files[0];
    if (merged == null || bundle.files.length !== 1) {
      throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
    }
    validateFirmwareFileForDevice(merged.fileName, merged.data, deviceInfo);
    return;
  }
  validateFirmwareUpdateFiles(bundle.files, deviceInfo);
}

async function fetchOfficialFirmwareForDevice(
  deviceInfo: FlashDeviceInfo,
  mode: FirmwareFlashMode,
): Promise<FirmwareFlashBundle> {
  const board = ensureSupportedFlashBoard(deviceInfo);
  const bundleBaseUrl = getFirmwareBundleBaseUrl();
  const catalogUrl = firmwareAssetUrl(bundleBaseUrl, "release-catalog.json");

  let catalogText = "";
  try {
    const catalogResponse = await fetch(catalogUrl, { cache: "no-store" });
    if (!catalogResponse.ok) {
      throw new Error(`catalog ${catalogResponse.status}`);
    }
    catalogText = await catalogResponse.text();
  } catch {
    throw new FlashDeviceError("device.flashFirmwareSourceUnavailable");
  }

  const catalogEntries = parseFirmwareCatalog(catalogText);
  const entry =
    catalogEntries.find((candidate) => candidate.boardId === board.value) ??
    null;
  if (entry == null) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  ensureCatalogEntryMatchesBoard(entry, board);

  if (mode === "reinstall") {
    const data = await fetchFirmwareAsset(bundleBaseUrl, entry.bin);
    const bundle = {
      boardId: board.value,
      files: [
        {
          address: FLASH_FIRMWARE_ADDRESS,
          data,
          fileName: entry.bin.file,
          kind: "merged" as const,
        },
      ],
      mode,
    };
    validateFirmwareBundleForDevice(bundle, deviceInfo);
    return bundle;
  }

  if (entry.updateParts.length === 0) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  const files: FirmwareFlashFile[] = [];
  for (const part of entry.updateParts) {
    files.push({
      address: part.offset,
      data: await fetchFirmwareAsset(bundleBaseUrl, part),
      fileName: part.file,
      kind: part.kind,
    });
  }
  const bundle = {
    boardId: board.value,
    files,
    mode,
  };
  validateFirmwareBundleForDevice(bundle, deviceInfo);
  return bundle;
}

function createEspLoader(port: SerialPortLike) {
  const transport = new Transport(
    port as unknown as ConstructorParameters<typeof Transport>[0],
    false,
  );
  const loader = new ESPLoader({
    transport,
    baudrate: FLASH_DEVICE_BAUD_RATE,
    terminal: FLASH_LOADER_TERMINAL,
    debugLogging: false,
  });
  return { loader, transport };
}

async function readFlashDeviceInfo(loader: ESPLoader): Promise<FlashDeviceInfo> {
  const chipDescription = await loader.main();
  const features = await loader.chip.getChipFeatures(loader);
  const flashSize = await loader.detectFlashSize();
  return {
    chipName: loader.chip.CHIP_NAME,
    chipDescription,
    flashSize,
    psramSize: extractMemoryFeature(features, "PSRAM"),
    features,
  };
}

async function inspectFlashDevice(port: SerialPortLike) {
  ensureSupportedUsbBridge(port);
  const { loader, transport } = createEspLoader(port);
  let deviceInfo: FlashDeviceInfo | null = null;
  let inspectError: FlashDeviceError | null = null;
  let releaseFailed = false;
  try {
    deviceInfo = await readFlashDeviceInfo(loader);
  } catch {
    inspectError = new FlashDeviceError("device.flashDeviceConnectFailed");
  } finally {
    try {
      await transport.disconnect();
    } catch {
      releaseFailed = true;
    }
  }
  if (inspectError != null) throw inspectError;
  if (releaseFailed) {
    throw new FlashDeviceError("device.flashDeviceReleaseFailed");
  }
  if (deviceInfo == null) {
    throw new FlashDeviceError("device.flashDeviceConnectFailed");
  }
  return deviceInfo;
}

async function flashFirmwareToDevice({
  bundle,
  eraseAll,
  onProgress,
  port,
}: {
  bundle: FirmwareFlashBundle;
  eraseAll: boolean;
  onProgress: (progress: number) => void;
  port: SerialPortLike;
}) {
  ensureSupportedUsbBridge(port);
  const { loader, transport } = createEspLoader(port);
  let flashError: FlashDeviceError | null = null;
  let releaseFailed = false;
  let resetIssued = false;
  let deviceInfo: FlashDeviceInfo | null = null;
  try {
    deviceInfo = await readFlashDeviceInfo(loader);
    validateFirmwareBundleForDevice(bundle, deviceInfo);
    const fileCount = Math.max(bundle.files.length, 1);
    const flashOptions: FlashOptions = {
      fileArray: bundle.files.map((file) => ({
        data: file.data,
        address: file.address,
      })),
      flashMode: "keep",
      flashFreq: "keep",
      flashSize: "keep",
      eraseAll,
      compress: true,
      reportProgress: (fileIndex, written, total) => {
        if (total > 0) {
          const fileProgress = Math.min(1, written / total);
          onProgress(
            Math.min(100, Math.floor(((fileIndex + fileProgress) / fileCount) * 100)),
          );
        }
      },
    };
    await loader.writeFlash(flashOptions);
    await loader.after("hard_reset");
    resetIssued = true;
  } catch (error) {
    flashError =
      error instanceof FlashDeviceError
        ? error
        : new FlashDeviceError("device.flashWriteFailed");
  } finally {
    try {
      await transport.disconnect();
    } catch {
      releaseFailed = true;
    }
  }
  if (flashError != null) throw flashError;
  if (releaseFailed && !resetIssued) {
    throw new FlashDeviceError("device.flashDeviceReleaseFailed");
  }
  if (deviceInfo == null) {
    throw new FlashDeviceError("device.flashWriteFailed");
  }
  return deviceInfo;
}

export interface FirmwareFlashDialogProps {
  open: boolean;
  onClose: () => void;
}

function WindowControls({
  closeLabel,
  disabled,
  onClose,
}: {
  closeLabel: string;
  disabled: boolean;
  onClose: () => void;
}) {
  const dots = [
    {
      key: "close",
      label: closeLabel,
      color: "var(--semantic-danger)",
      ink: "color-mix(in srgb, var(--semantic-danger) 62%, #491316)",
      icon: (
        <>
          <path d="M4 4L10 10" />
          <path d="M10 4L4 10" />
        </>
      ),
    },
    {
      key: "minimize",
      label: "",
      color: "var(--semantic-warning)",
      ink: "color-mix(in srgb, var(--semantic-warning) 62%, #563600)",
      icon: <path d="M4 7H10" />,
    },
    {
      key: "zoom",
      label: "",
      color: "var(--semantic-success)",
      ink: "color-mix(in srgb, var(--semantic-success) 62%, #0b3e22)",
      icon: (
        <>
          <path d="M7 3.8V10.2" />
          <path d="M3.8 7H10.2" />
        </>
      ),
    },
  ];

  return (
    <Box
      sx={{
        display: "flex",
        alignItems: "center",
        gap: 0.75,
        "&:hover .window-control-symbol": {
          opacity: 1,
        },
      }}
    >
      {dots.map((dot, index) => (
        <Box
          key={dot.key}
          component={index === 0 ? "button" : "span"}
          type={index === 0 ? "button" : undefined}
          aria-label={index === 0 ? dot.label : undefined}
          aria-hidden={index === 0 ? undefined : true}
          disabled={index === 0 ? disabled : undefined}
          onClick={index === 0 ? onClose : undefined}
          sx={{
            width: 16,
            height: 16,
            p: 0,
            border: "1px solid color-mix(in srgb, var(--foreground) 10%, transparent)",
            borderRadius: "50%",
            backgroundColor: dot.color,
            backgroundImage:
              "linear-gradient(180deg, color-mix(in srgb, #fff 54%, transparent) 0%, transparent 62%)",
            boxShadow:
              "inset 0 1px 0 color-mix(in srgb, #fff 54%, transparent), 0 1px 2px color-mix(in srgb, var(--foreground) 12%, transparent)",
            cursor: index === 0 && !disabled ? "pointer" : "default",
            appearance: "none",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: dot.ink,
            lineHeight: 1,
          }}
        >
          <Box
            component="svg"
            viewBox="0 0 14 14"
            className="window-control-symbol"
            sx={{
              width: 14,
              height: 14,
              opacity: 0,
              transition: "opacity var(--transition-duration) ease",
              "& path": {
                fill: "none",
                stroke: "currentColor",
                strokeWidth: 1.9,
                strokeLinecap: "round",
              },
            }}
          >
            {dot.icon}
          </Box>
        </Box>
      ))}
    </Box>
  );
}

export function FirmwareFlashDialog({ open, onClose }: FirmwareFlashDialogProps) {
  const { t } = useTranslation();
  const { showToast } = useToast();
  const [serialDevices, setSerialDevices] = useState<FlashDeviceEntry[]>([]);
  const [serialPortIndex, setSerialPortIndex] = useState("");
  const [serialScanning, setSerialScanning] = useState(false);
  const [serialError, setSerialError] = useState("");
  const [flashMode, setFlashMode] = useState<FirmwareFlashMode>("update");
  const [running, setRunning] = useState(false);
  const [flashProgress, setFlashProgress] = useState<number | null>(null);

  const supportsUsb = useMemo(() => {
    return getSerialApi() != null;
  }, []);

  const closeModal = () => {
    if (!running) onClose();
  };

  const handleDialogClose = (
    _event: object,
    reason: Exclude<FirmwareFlashCloseReason, "explicit">,
  ) => {
    if (!shouldCloseFirmwareFlashDialog(reason)) return;
    closeModal();
  };

  const handleSerialPortChange = async (event: SelectChangeEvent<string>) => {
    const nextIndex = event.target.value;
    const nextDevice = serialDevices[Number(nextIndex)];
    setSerialPortIndex((current) =>
      nextFlashDeviceSelectionAfterFailure(current, "selectDevice"),
    );
    setSerialError("");
    if (!nextDevice) return;

    setSerialScanning(true);
    try {
      const info = await inspectFlashDevice(nextDevice.port);
      ensureSupportedFlashBoard(info);
      setSerialDevices((current) =>
        current.map((item, index) =>
          index === Number(nextIndex) ? { ...item, info } : item,
        ),
      );
      setSerialPortIndex(nextIndex);
    } catch (error) {
      setSerialError(flashDeviceErrorMessage(error, t));
    } finally {
      setSerialScanning(false);
    }
  };

  const handleFlashModeChange = (
    _event: MouseEvent<HTMLElement>,
    nextMode: FirmwareFlashMode | null,
  ) => {
    if (nextMode == null || running) return;
    setFlashMode(nextMode);
  };

  const handleScanSerial = async () => {
    const serial = getSerialApi();
    if (serial == null) {
      showToast(t("device.flashSerialUnsupported"), { variant: "error" });
      return;
    }

    setSerialScanning(true);
    setSerialError("");
    setSerialDevices([]);
    setSerialPortIndex("");
    try {
      const port = await serial.requestPort({ filters: FLASH_DEVICE_FILTERS });
      const info = await inspectFlashDevice(port);
      ensureSupportedFlashBoard(info);
      setSerialDevices([{ port, info }]);
      setSerialPortIndex("0");
    } catch (error) {
      if ((error as { name?: string }).name !== "NotFoundError") {
        setSerialError(flashDeviceErrorMessage(error, t));
      }
    } finally {
      setSerialScanning(false);
    }
  };

  const handleSubmit = async () => {
    if (!supportsUsb) {
      showToast(t("device.flashSerialUnsupported"), { variant: "error" });
      return;
    }
    const selectedDevice =
      serialPortIndex === ""
        ? null
        : serialDevices[Number(serialPortIndex)] ?? null;
    if (selectedDevice == null) {
      showToast(t("device.flashNeedSerial"), { variant: "warning" });
      return;
    }

    setRunning(true);
    setFlashProgress(0);
    setSerialError("");
    try {
      const firmware = await fetchOfficialFirmwareForDevice(
        selectedDevice.info,
        flashMode,
      );
      const info = await flashFirmwareToDevice({
        bundle: firmware,
        eraseAll: shouldEraseBeforeFirmwareFlash(flashMode),
        onProgress: setFlashProgress,
        port: selectedDevice.port,
      });
      setSerialDevices((current) =>
        current.map((item, index) =>
          index === Number(serialPortIndex) ? { ...item, info } : item,
        ),
      );
      setFlashProgress(100);
      showToast(t("device.flashSucceeded"), { variant: "success" });
    } catch (error) {
      setSerialPortIndex((current) =>
        nextFlashDeviceSelectionAfterFailure(current, "submitFlash"),
      );
      setSerialError(flashDeviceErrorMessage(error, t));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={handleDialogClose}
      disableEscapeKeyDown
      maxWidth={false}
      aria-labelledby={FLASH_DIALOG_TITLE_ID}
      slotProps={{
        backdrop: {
          sx: {
            backgroundColor: "var(--backdrop-overlay)",
            backdropFilter: "blur(var(--glass-blur))",
            WebkitBackdropFilter: "blur(var(--glass-blur))",
          },
        },
        paper: {
          sx: {
            width: "min(560px, calc(100vw - 24px))",
            borderRadius: "var(--radius-card)",
            border: "1px solid var(--form-outline-rest)",
            backgroundColor: "var(--card)",
            boxShadow: "var(--os3d-content-plate-stack)",
            overflow: "hidden",
          },
        },
      }}
    >
      <DialogTitle
        component="div"
        sx={{
          minHeight: 62,
          px: 2.25,
          py: 1.25,
          borderBottom: "1px solid color-mix(in srgb, var(--border) 12%, transparent)",
          display: "flex",
          alignItems: "center",
          gap: 1.35,
          backgroundColor: "var(--card)",
        }}
      >
        <WindowControls
          closeLabel={t("device.flashModalClose")}
          disabled={running}
          onClose={closeModal}
        />
        <Typography
          id={FLASH_DIALOG_TITLE_ID}
          component="h2"
          sx={{
            color: "var(--text-primary)",
            fontFamily: "var(--font-brand)",
            fontSize: "var(--font-size-body-sm)",
            fontWeight: 700,
            lineHeight: "var(--line-height-tight)",
          }}
        >
          {t("device.flashModalTitle")}
        </Typography>
      </DialogTitle>

      <DialogContent
        sx={{
          p: 0,
          backgroundColor: "var(--form-group-well)",
        }}
      >
        {!supportsUsb ? (
          <Box sx={{ p: 3 }}>
            <Box
              sx={{
                p: 2,
                borderRadius: "var(--radius-control)",
                border:
                  "1px solid color-mix(in srgb, var(--semantic-danger) 24%, transparent)",
                backgroundColor:
                  "color-mix(in srgb, var(--semantic-danger) 7%, var(--card))",
                boxShadow: "var(--os3d-control-soft-lift-stack)",
              }}
            >
              <Typography
                sx={{
                  color: "var(--text-primary)",
                  fontSize: "var(--font-size-body-sm)",
                  fontWeight: 800,
                }}
              >
                {t("device.flashSerialUnsupported")}
              </Typography>
            </Box>
            <Box sx={{ mt: 3, display: "flex", justifyContent: "flex-end" }}>
              <Button variant="contained" onClick={closeModal}>
                {t("common.cancel")}
              </Button>
            </Box>
          </Box>
        ) : (
          <Stack
            spacing={0}
            sx={{
              width: "100%",
              display: "flex",
              flexDirection: "column",
            }}
          >
            <Stack
              spacing={0}
              sx={{
                ...DIALOG_FORM_SCROLL_WELL_SX,
                px: { xs: 2.5, sm: 4 },
                py: { xs: 3, sm: 4 },
              }}
            >
              <FormGrid columns={1} sx={{ gap: { xs: 3, sm: 3.5 } }}>
                <Box
                  sx={{
                    display: "grid",
                    gridTemplateColumns: { xs: "1fr", sm: "minmax(0, 1fr) auto" },
                    gap: 2,
                    alignItems: "start",
                  }}
                >
                  <FormControl fullWidth error={Boolean(serialError)}>
                    <InputLabel id="firmware-flash-serial-label">
                      {t("device.flashSerialLabel")}
                    </InputLabel>
                    <Select
                      labelId="firmware-flash-serial-label"
                      value={serialPortIndex}
                      label={t("device.flashSerialLabel")}
                      onChange={handleSerialPortChange}
                      disabled={
                        running || serialScanning || serialDevices.length === 0
                      }
                    >
                      {serialDevices.length === 0 ? (
                        <MenuItem value="" disabled>
                          {t("device.flashNoSerial")}
                        </MenuItem>
                      ) : (
                        serialDevices.map((device, index) => (
                          <MenuItem
                            key={`${formatFlashDeviceLabel(device.info)}-${index}`}
                            value={String(index)}
                          >
                            {formatFlashDeviceLabel(device.info)}
                          </MenuItem>
                        ))
                      )}
                    </Select>
                    {serialError ? (
                      <FormHelperText>{serialError}</FormHelperText>
                    ) : null}
                  </FormControl>
                  <Button
                    variant="outlined"
                    size="large"
                    onClick={handleScanSerial}
                    disabled={running || serialScanning}
                    startIcon={
                      serialScanning ? (
                        <CircularProgress size={18} color="inherit" />
                      ) : undefined
                    }
                    sx={{
                      height: 56,
                      minWidth: { xs: "100%", sm: 128 },
                    }}
                  >
                    {serialScanning
                      ? t("device.flashDeviceConnecting")
                      : t("device.flashScanSerial")}
                  </Button>
                </Box>

                <Box>
                  <Typography
                    sx={{
                      mb: 1,
                      color: "var(--text-secondary)",
                      fontSize: "var(--font-size-caption)",
                      fontWeight: 700,
                    }}
                  >
                    {t("device.flashModeLabel")}
                  </Typography>
                  <ToggleButtonGroup
                    exclusive
                    fullWidth
                    value={flashMode}
                    onChange={handleFlashModeChange}
                    disabled={running}
                    aria-label={t("device.flashModeLabel")}
                  >
                    <ToggleButton value="update">
                      {t("device.flashModeUpdate")}
                    </ToggleButton>
                    <ToggleButton value="reinstall">
                      {t("device.flashModeReinstall")}
                    </ToggleButton>
                  </ToggleButtonGroup>
                  <FormHelperText
                    sx={{
                      mx: 0,
                      mt: 1,
                      color:
                        flashMode === "reinstall"
                          ? "var(--semantic-danger)"
                          : "var(--text-secondary)",
                    }}
                  >
                    {flashMode === "reinstall"
                      ? t("device.flashModeReinstallHint")
                      : t("device.flashModeUpdateHint")}
                  </FormHelperText>
                </Box>
              </FormGrid>
            </Stack>

            <Box
              sx={{
                ...DIALOG_FORM_SUBMIT_BAR_SX,
              }}
            >
              <Button
                fullWidth
                size="large"
                variant="contained"
                onClick={handleSubmit}
                startIcon={
                  running ? (
                    <CircularProgress size={18} color="inherit" />
                  ) : undefined
                }
                disabled={
                  running ||
                  serialScanning ||
                  serialPortIndex === "" ||
                  Boolean(serialError)
                }
              >
                {running && flashProgress != null
                  ? t("device.flashRunningWithProgress", {
                      progress: flashProgress,
                    })
                  : running
                    ? t("device.flashRunning")
                    : t("device.flashStart")}
              </Button>
            </Box>
          </Stack>
        )}
      </DialogContent>
    </Dialog>
  );
}

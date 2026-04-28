import { type ChangeEvent, useMemo, useRef, useState } from "react";
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
import TextField from "@mui/material/TextField";
import Typography from "@mui/material/Typography";
import { useToast } from "../hooks/useToast";
import {
  DIALOG_FORM_SCROLL_WELL_SX,
  DIALOG_FORM_SUBMIT_BAR_SX,
} from "../theme/panelStyles";
import { FormGrid } from "./form";

type FlashBoard = {
  value: string;
  label: string;
  chipName: string;
  flashSize: string;
  minPsramSize?: string;
};

type FlashDeviceInfo = {
  chipName: string;
  chipDescription: string;
  flashSize: string;
  psramSize: string | null;
  features: string[];
};

type FlashDeviceEntry = {
  port: SerialPortLike;
  info: FlashDeviceInfo;
};

type FirmwareCatalogEntry = {
  boardId: string;
  file: string;
  sha256: string;
  sizeBytes: number | null;
  flashSize: string | null;
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

const FLASH_BOARD_OPTIONS: FlashBoard[] = [
  {
    value: "esp32-s3-8mb",
    label: "ESP32-S3 8MB",
    chipName: "ESP32-S3",
    flashSize: "8MB",
    minPsramSize: "8MB",
  },
  {
    value: "esp32-s3-16mb",
    label: "ESP32-S3 16MB",
    chipName: "ESP32-S3",
    flashSize: "16MB",
    minPsramSize: "8MB",
  },
  {
    value: "esp32-s3-32mb",
    label: "ESP32-S3 32MB",
    chipName: "ESP32-S3",
    flashSize: "32MB",
    minPsramSize: "16MB",
  },
  {
    value: "esp32-p4-nano-16mb",
    label: "ESP32-P4 Nano 16MB",
    chipName: "ESP32-P4",
    flashSize: "16MB",
  },
];

const FLASH_DIALOG_TITLE_ID = "firmware-flash-dialog-title";
const FLASH_DEVICE_BAUD_RATE = 115_200;
const FLASH_FIRMWARE_ADDRESS = 0x0;
const ESP_IMAGE_MAGIC = 0xe9;
const ESP_PARTITION_TABLE_OFFSET = 0x8000;
const ESP_APP_IMAGE_OFFSET = 0x20000;
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

function normalizeMemorySize(size: string): string {
  return size.replace(/\s+/g, "").toUpperCase();
}

function formatMemorySize(size: string): string {
  return normalizeMemorySize(size).replace(/(\d+(?:\.\d+)?)(KB|MB)$/u, "$1 $2");
}

function parseMemorySizeBytes(size: string): number | null {
  const match = normalizeMemorySize(size).match(/^(\d+(?:\.\d+)?)(KB|MB)$/u);
  if (match?.[1] == null || match[2] == null) return null;
  const value = Number(match[1]);
  if (!Number.isFinite(value) || value <= 0) return null;
  return Math.floor(value * (match[2] === "MB" ? 1024 * 1024 : 1024));
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

function extractMemoryFeature(features: string[], kind: "Flash" | "PSRAM") {
  const matcher = new RegExp(`${kind}\\s+(\\d+(?:\\.\\d+)?)\\s*(KB|MB)`, "iu");
  for (const feature of features) {
    const match = feature.match(matcher);
    if (match?.[1] != null && match[2] != null) {
      return `${match[1]}${match[2].toUpperCase()}`;
    }
  }
  return null;
}

function formatFlashDeviceLabel(info: FlashDeviceInfo): string {
  const parts = [
    info.chipDescription,
    `Flash ${formatMemorySize(info.flashSize)}`,
  ];
  if (info.psramSize != null) {
    parts.push(`PSRAM ${formatMemorySize(info.psramSize)}`);
  }
  return parts.join(" · ");
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

function resolveFlashBoard(info: FlashDeviceInfo): FlashBoard | null {
  return (
    FLASH_BOARD_OPTIONS.find(
      (board) =>
        board.chipName === info.chipName &&
        normalizeMemorySize(board.flashSize) === normalizeMemorySize(info.flashSize),
    ) ?? null
  );
}

function ensureSupportedFlashBoard(info: FlashDeviceInfo) {
  const board = resolveFlashBoard(info);
  if (board == null) {
    throw new FlashDeviceError("device.flashDeviceUnsupportedBoard", {
      actual: formatFlashDeviceLabel(info),
    });
  }
  if (board.minPsramSize != null) {
    const detectedPsramBytes =
      info.psramSize == null ? null : parseMemorySizeBytes(info.psramSize);
    const requiredPsramBytes = parseMemorySizeBytes(board.minPsramSize);
    if (
      detectedPsramBytes == null ||
      requiredPsramBytes == null ||
      detectedPsramBytes < requiredPsramBytes
    ) {
      throw new FlashDeviceError("device.flashDeviceUnsupportedBoard", {
        actual: formatFlashDeviceLabel(info),
      });
    }
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

function hasPartitionTableMagic(data: Uint8Array): boolean {
  const first = data[ESP_PARTITION_TABLE_OFFSET];
  const second = data[ESP_PARTITION_TABLE_OFFSET + 1];
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
    const bin = board.bin;
    if (boardId == null || !isRecord(bin)) continue;
    const file = readStringField(bin, "file");
    const rawSha256 = readStringField(bin, "sha256");
    const sha256 = rawSha256 == null ? null : normalizeSha256(rawSha256);
    if (file == null || sha256 == null) continue;
    entries.push({
      boardId,
      file,
      sha256,
      sizeBytes: readNumberField(bin, "size_bytes"),
      flashSize,
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

async function validateFirmwareCatalogForFile({
  catalogText,
  data,
  fileName,
}: {
  catalogText: string;
  data: Uint8Array;
  fileName: string;
}) {
  const firmwareBoard = inferFirmwareBoardFromName(fileName);
  if (firmwareBoard == null) {
    throw new FlashDeviceError("device.flashFirmwareInvalid");
  }
  const catalogEntries = parseFirmwareCatalog(catalogText);
  const entry =
    catalogEntries.find(
      (candidate) =>
        candidate.boardId === firmwareBoard.value && candidate.file === fileName,
    ) ?? null;
  if (entry == null) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  if (
    entry.flashSize != null &&
    normalizeMemorySize(entry.flashSize) !==
      normalizeMemorySize(firmwareBoard.flashSize)
  ) {
    throw new FlashDeviceError("device.flashFirmwareCatalogInvalid");
  }
  if (entry.sizeBytes != null && entry.sizeBytes !== data.byteLength) {
    throw new FlashDeviceError("device.flashFirmwareChecksumMismatch");
  }
  const actualSha256 = await sha256Hex(data);
  if (actualSha256 !== entry.sha256) {
    throw new FlashDeviceError("device.flashFirmwareChecksumMismatch");
  }
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
  data,
  fileName,
  onProgress,
  port,
}: {
  data: Uint8Array;
  fileName: string;
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
    validateFirmwareFileForDevice(fileName, data, deviceInfo);
    const flashOptions: FlashOptions = {
      fileArray: [{ data, address: FLASH_FIRMWARE_ADDRESS }],
      flashMode: "keep",
      flashFreq: "keep",
      flashSize: "keep",
      eraseAll: false,
      compress: true,
      reportProgress: (_fileIndex, written, total) => {
        if (total > 0) {
          onProgress(Math.min(100, Math.floor((written / total) * 100)));
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
  const [firmwareFile, setFirmwareFile] = useState<File | null>(null);
  const [firmwareName, setFirmwareName] = useState("");
  const [catalogFile, setCatalogFile] = useState<File | null>(null);
  const [catalogName, setCatalogName] = useState("");
  const [running, setRunning] = useState(false);
  const [flashProgress, setFlashProgress] = useState<number | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const catalogInputRef = useRef<HTMLInputElement | null>(null);

  const supportsUsb = useMemo(() => {
    return getSerialApi() != null;
  }, []);

  const closeModal = () => {
    if (!running) onClose();
  };

  const handleSerialPortChange = async (event: SelectChangeEvent<string>) => {
    const nextIndex = event.target.value;
    const nextDevice = serialDevices[Number(nextIndex)];
    setSerialPortIndex("");
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

  const handleFileInput = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.currentTarget.files?.[0] ?? null;
    setFlashProgress(null);
    if (file != null && inferFirmwareBoardFromName(file.name) == null) {
      event.currentTarget.value = "";
      setFirmwareFile(null);
      setFirmwareName("");
      showToast(t("device.flashFirmwareInvalid"), { variant: "error" });
      return;
    }
    setFirmwareFile(file);
    setFirmwareName(file?.name ?? "");
  };

  const handleCatalogInput = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.currentTarget.files?.[0] ?? null;
    setFlashProgress(null);
    if (file != null && !file.name.trim().toLowerCase().endsWith(".json")) {
      event.currentTarget.value = "";
      setCatalogFile(null);
      setCatalogName("");
      showToast(t("device.flashFirmwareCatalogInvalid"), { variant: "error" });
      return;
    }
    setCatalogFile(file);
    setCatalogName(file?.name ?? "");
  };

  const handleChooseFirmware = () => {
    if (!running) fileInputRef.current?.click();
  };

  const handleChooseCatalog = () => {
    if (!running) catalogInputRef.current?.click();
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
    const serialPort =
      serialPortIndex === ""
        ? null
        : serialDevices[Number(serialPortIndex)]?.port ?? null;
    if (serialPort == null) {
      showToast(t("device.flashNeedSerial"), { variant: "warning" });
      return;
    }
    if (firmwareFile == null) {
      showToast(t("device.flashNeedFile"), { variant: "warning" });
      return;
    }
    if (catalogFile == null) {
      showToast(t("device.flashNeedCatalog"), { variant: "warning" });
      return;
    }

    setRunning(true);
    setFlashProgress(0);
    setSerialError("");
    try {
      const data = new Uint8Array(await firmwareFile.arrayBuffer());
      await validateFirmwareCatalogForFile({
        catalogText: await catalogFile.text(),
        data,
        fileName: firmwareFile.name,
      });
      const info = await flashFirmwareToDevice({
        data,
        fileName: firmwareFile.name,
        onProgress: setFlashProgress,
        port: serialPort,
      });
      setSerialDevices((current) =>
        current.map((item, index) =>
          index === Number(serialPortIndex) ? { ...item, info } : item,
        ),
      );
      setFlashProgress(100);
      showToast(t("device.flashSucceeded"), { variant: "success" });
    } catch (error) {
      setSerialPortIndex("");
      setSerialError(flashDeviceErrorMessage(error, t));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={closeModal}
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

                <Button
                  component="div"
                  disabled={running}
                  onClick={handleChooseFirmware}
                  sx={{ p: 0, textAlign: "left", textTransform: "none" }}
                >
                  <TextField
                    fullWidth
                    label={t("device.flashFirmwareLabel")}
                    value={
                      firmwareName || t("device.flashChooseFirmwareInline")
                    }
                    InputProps={{ readOnly: true }}
                    inputProps={{
                      tabIndex: -1,
                      sx: { cursor: running ? "default" : "pointer" },
                    }}
                    sx={{
                      pointerEvents: "none",
                      cursor: running ? "default" : "pointer",
                    }}
                  />
                </Button>
                <Box
                  ref={fileInputRef}
                  component="input"
                  type="file"
                  accept=".bin"
                  hidden
                  onChange={handleFileInput}
                />
                <Button
                  component="div"
                  disabled={running}
                  onClick={handleChooseCatalog}
                  sx={{ p: 0, textAlign: "left", textTransform: "none" }}
                >
                  <TextField
                    fullWidth
                    label={t("device.flashCatalogLabel")}
                    value={catalogName || t("device.flashChooseCatalogInline")}
                    InputProps={{ readOnly: true }}
                    inputProps={{
                      tabIndex: -1,
                      sx: { cursor: running ? "default" : "pointer" },
                    }}
                    sx={{
                      pointerEvents: "none",
                      cursor: running ? "default" : "pointer",
                    }}
                  />
                </Button>
                <Box
                  ref={catalogInputRef}
                  component="input"
                  type="file"
                  accept=".json"
                  hidden
                  onChange={handleCatalogInput}
                />
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
                  firmwareFile == null ||
                  catalogFile == null ||
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

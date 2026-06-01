import test from "node:test";
import assert from "node:assert/strict";
import {
  FLASH_BOARD_OPTIONS,
  extractMemoryFeature,
  flashPartsTouchPreservedRanges,
  firmwareFlashRequiredUpdatePartKinds,
  formatFlashDeviceLabel,
  isFirmwareFlashUpdatePartKind,
  nextFlashDeviceSelectionAfterFailure,
  resolveFlashBoard,
  shouldCloseFirmwareFlashDialog,
  shouldEraseBeforeFirmwareFlash,
  type FlashDeviceInfo,
} from "./firmwareFlashModel.ts";

function deviceInfo(overrides: Partial<FlashDeviceInfo>): FlashDeviceInfo {
  return {
    chipName: "ESP32-S3",
    chipDescription: "ESP32-S3 (QFN56) (revision v0.2)",
    flashSize: "16MB",
    psramSize: null,
    features: ["Wi-Fi", "BLE"],
    ...overrides,
  };
}

test("resolveFlashBoard maps official S3 16MB without requiring ROM-stage PSRAM detection", () => {
  const board = resolveFlashBoard(deviceInfo({ psramSize: null }));

  assert.equal(board?.value, "esp32-s3-16mb");
});

test("resolveFlashBoard still rejects unsupported Flash sizes", () => {
  const board = resolveFlashBoard(deviceInfo({ flashSize: "4MB" }));

  assert.equal(board, null);
});

test("resolveFlashBoard keeps chip family in the board match", () => {
  const board = resolveFlashBoard(
    deviceInfo({
      chipName: "ESP32-C3",
      chipDescription: "ESP32-C3",
      flashSize: "16MB",
    }),
  );

  assert.equal(board, null);
});

test("formatFlashDeviceLabel shows PSRAM only when the loader reports it", () => {
  assert.equal(
    formatFlashDeviceLabel(deviceInfo({ psramSize: null })),
    "ESP32-S3 (QFN56) (revision v0.2) · Flash 16 MB",
  );
  assert.equal(
    formatFlashDeviceLabel(deviceInfo({ psramSize: "8MB" })),
    "ESP32-S3 (QFN56) (revision v0.2) · Flash 16 MB · PSRAM 8 MB",
  );
});

test("extractMemoryFeature parses esptool-js feature strings", () => {
  assert.equal(
    extractMemoryFeature(["Embedded PSRAM 8MB (AP_3v3)"], "PSRAM"),
    "8MB",
  );
  assert.equal(
    extractMemoryFeature(["Embedded Flash 16MB (GD)"], "Flash"),
    "16MB",
  );
});

test("shouldCloseFirmwareFlashDialog only allows explicit close button actions", () => {
  assert.equal(shouldCloseFirmwareFlashDialog("explicit"), true);
  assert.equal(shouldCloseFirmwareFlashDialog("backdropClick"), false);
  assert.equal(shouldCloseFirmwareFlashDialog("escapeKeyDown"), false);
});

test("shouldEraseBeforeFirmwareFlash maps reinstall to full-chip erase only", () => {
  assert.equal(shouldEraseBeforeFirmwareFlash("update"), false);
  assert.equal(shouldEraseBeforeFirmwareFlash("reinstall"), true);
});

test("firmware update parts include WakeNet model for supported release boards", () => {
  assert.equal(isFirmwareFlashUpdatePartKind("model"), true);
  assert.equal(isFirmwareFlashUpdatePartKind("storage"), false);

  for (const board of FLASH_BOARD_OPTIONS) {
    assert.equal(board.requiresModelPartition, true);
    assert.deepEqual(firmwareFlashRequiredUpdatePartKinds(board), [
      "bootloader",
      "partition-table",
      "app",
      "model",
    ]);
  }
});

test("nextFlashDeviceSelectionAfterFailure keeps device selection on submit failures", () => {
  assert.equal(nextFlashDeviceSelectionAfterFailure("0", "submitFlash"), "0");
  assert.equal(nextFlashDeviceSelectionAfterFailure("0", "selectDevice"), "");
});

test("flashPartsTouchPreservedRanges detects update plans that would write NVS", () => {
  const nvsRange = { start: 0x9000, end: 0x19000 };

  assert.equal(
    flashPartsTouchPreservedRanges(
      [
        { address: 0x0, sizeBytes: 0x7000 },
        { address: 0x8000, sizeBytes: 0x1000 },
        { address: 0x20000, sizeBytes: 0x1000 },
      ],
      [nvsRange],
    ),
    false,
  );
  assert.equal(
    flashPartsTouchPreservedRanges(
      [{ address: 0x0, sizeBytes: 0x553910 }],
      [nvsRange],
    ),
    true,
  );
});

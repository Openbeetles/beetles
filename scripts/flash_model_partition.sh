#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PARTITION_CSV="${PARTITION_CSV:-$ROOT/partitions.csv}"
MODEL_BIN="${MODEL_BIN:-}"
PORT="${ESPFLASH_PORT:-${ESPTOOL_PORT:-}}"
CHIP="${ESPTOOL_CHIP:-esp32s3}"

usage() {
  cat <<'EOF'
Usage:
  scripts/flash_model_partition.sh --port /dev/tty.usbmodemXXXX [--bin path/to/srmodels.bin] [--partition-csv partitions.csv]

Behavior:
  - Writes only the `model` partition.
  - Does NOT erase the whole chip.
  - Does NOT touch the `spiffs` partition.

Environment:
  ESPFLASH_PORT / ESPTOOL_PORT  Serial port fallback
  ESPTOOL_CHIP                  Chip name, default: esp32s3
  MODEL_BIN                     Model image fallback
  PARTITION_CSV                 Partition table fallback
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)
      PORT="${2:-}"
      shift 2
      ;;
    --bin)
      MODEL_BIN="${2:-}"
      shift 2
      ;;
    --partition-csv)
      PARTITION_CSV="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$PORT" ]]; then
  echo "Missing serial port. Pass --port or set ESPFLASH_PORT." >&2
  exit 1
fi

if [[ ! -f "$PARTITION_CSV" ]]; then
  echo "Partition table not found: $PARTITION_CSV" >&2
  exit 1
fi

if [[ -z "$MODEL_BIN" ]]; then
  if [[ -f "$ROOT/artifacts/wake_models/srmodels_hiesp.bin" ]]; then
    MODEL_BIN="$ROOT/artifacts/wake_models/srmodels_hiesp.bin"
  else
    MODEL_BIN="$(find "$ROOT/target" -path '*out/build/srmodels/srmodels.bin' | head -n 1 || true)"
  fi
fi

if [[ -z "$MODEL_BIN" || ! -f "$MODEL_BIN" ]]; then
  echo "Model image not found. Pass --bin or build the ESP target first." >&2
  exit 1
fi

MODEL_ROW="$(awk -F',' '
  /^[[:space:]]*#/ { next }
  {
    name=$1
    gsub(/[[:space:]]/, "", name)
    if (name == "model") {
      offset=$4
      size=$5
      gsub(/[[:space:]]/, "", offset)
      gsub(/[[:space:]]/, "", size)
      print offset " " size
      exit
    }
  }
' "$PARTITION_CSV")"

if [[ -z "$MODEL_ROW" ]]; then
  echo "No model partition found in $PARTITION_CSV" >&2
  exit 1
fi

MODEL_OFFSET="${MODEL_ROW%% *}"
MODEL_SIZE="${MODEL_ROW##* }"

python3 - "$MODEL_BIN" "$MODEL_SIZE" <<'PY'
import pathlib
import sys

image = pathlib.Path(sys.argv[1])
size_text = sys.argv[2].strip().lower()
if size_text.startswith("0x"):
    part_size = int(size_text, 16)
else:
    part_size = int(size_text, 10)
img_size = image.stat().st_size
if img_size > part_size:
    raise SystemExit(f"model image too large: {img_size} > partition size {part_size}")
print(f"model image size: {img_size} bytes; partition size: {part_size} bytes")
PY

echo "Writing only model partition"
echo "  Port:            $PORT"
echo "  Chip:            $CHIP"
echo "  Partition table: $PARTITION_CSV"
echo "  Model image:     $MODEL_BIN"
echo "  Model offset:    $MODEL_OFFSET"
echo "  Model size:      $MODEL_SIZE"
echo "  Preserved:       spiffs, nvs, ota slots"

python3 -m esptool --chip "$CHIP" --port "$PORT" write_flash "$MODEL_OFFSET" "$MODEL_BIN"

#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/esp_symbolize_panic.sh <artifact-dir> <addresses...>

Example:
  scripts/esp_symbolize_panic.sh target/esp-artifacts/<artifact-id> 0x4037f815

Artifact hint:
  ESP build artifacts are written by TARGET=esp ./build.sh --no-deploy under:
    target/esp-artifacts/<git-sha>-<elf-sha>/

Panic log parser:
  scripts/parse_esp_panic_log.sh \
    --artifact-dir target/esp-artifacts/<artifact-id> \
    target/esp-soak/<run>/serial.log
EOF
}

if [[ $# -lt 2 ]]; then
  usage
  exit 2
fi

ARTIFACT_DIR="$1"
shift
ELF="$ARTIFACT_DIR/beetle.elf"
if [[ ! -f "$ELF" ]]; then
  ELF="$ARTIFACT_DIR/libespidf.elf"
fi
META="$ARTIFACT_DIR/artifact.env"

if [[ ! -d "$ARTIFACT_DIR" ]]; then
  echo "Error: artifact directory not found: $ARTIFACT_DIR" >&2
  exit 1
fi
if [[ ! -f "$ELF" ]]; then
  echo "Error: neither beetle.elf nor libespidf.elf was found in artifact directory: $ARTIFACT_DIR" >&2
  echo "Use the artifact id printed by TARGET=esp ./build.sh --no-deploy." >&2
  echo "If you only have a captured serial log, first run:" >&2
  echo "  scripts/parse_esp_panic_log.sh --artifact-dir $ARTIFACT_DIR <serial-log>" >&2
  exit 1
fi

ADDR2LINE="${ADDR2LINE:-}"
activate_esp_toolchain_path() {
  local export_script
  for export_script in "$HOME/export-esp.sh" "$HOME/.espup/export-esp.sh" "$HOME/.local/share/esp-rs/export-esp.sh"; do
    if [[ -f "$export_script" ]]; then
      # shellcheck source=/dev/null
      source "$export_script"
      return 0
    fi
  done

  local bin_dir
  for bin_dir in "$HOME/.rustup/toolchains/esp/"*/bin "$HOME/.rustup/toolchains/esp/xtensa-esp-elf/"*/xtensa-esp-elf/bin; do
    if [[ -x "$bin_dir/xtensa-esp32s3-elf-addr2line" || -x "$bin_dir/riscv32-esp-elf-addr2line" ]]; then
      export PATH="$bin_dir:$PATH"
      return 0
    fi
  done
}

if [[ -z "$ADDR2LINE" ]]; then
  activate_esp_toolchain_path || true
  for candidate in xtensa-esp32s3-elf-addr2line xtensa-esp32-elf-addr2line xtensa-esp-elf-addr2line riscv32-esp-elf-addr2line; do
    if command -v "$candidate" >/dev/null 2>&1; then
      ADDR2LINE="$candidate"
      break
    fi
  done
fi

if [[ -z "$ADDR2LINE" ]]; then
  echo "Error: ESP addr2line tool not found." >&2
  echo "Install or activate the ESP toolchain, for example:" >&2
  echo "  espup install" >&2
  echo "  . ~/export-esp.sh" >&2
  echo "Or set ADDR2LINE=/path/to/xtensa-esp32s3-elf-addr2line." >&2
  exit 1
fi

if [[ -f "$META" ]]; then
  artifact_id="$(sed -n 's/^artifact_id=//p' "$META" | head -n1)"
  elf_sha="$(sed -n 's/^elf_sha256=//p' "$META" | head -n1)"
  partition_sha="$(sed -n 's/^partition_table_sha256=//p' "$META" | head -n1)"
  echo "Artifact: ${artifact_id:-unknown}" >&2
  echo "ELF SHA256: ${elf_sha:-unknown}" >&2
  echo "Partition SHA256: ${partition_sha:-unknown}" >&2
fi

"$ADDR2LINE" -pfiaC -e "$ELF" "$@"

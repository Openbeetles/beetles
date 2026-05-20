#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/esp_profile_matrix.sh [options]

Options:
  --profile ROW        Matrix row: baseline, alwaysinternal_0, alwaysinternal_1024,
                       alwaysinternal_2048, xip_psram_candidate.
  --artifact PATH      Artifact directory or id to record.
  --log PATH           Captured serial log to record and analyze.
  --operator NAME      Operator name or handle. Default: current user.
  --board NAME         Board label. Default: esp32s3.
  --evidence-dir DIR   Evidence root. Default: target/esp-profile-matrix.
  --heap-largest-floor BYTES
                       Floor passed to esp_soak_analyze.sh. Default: 32768.
  --init-only          Create skeleton evidence files and print commands.

This script records and prints experiment evidence only. It never edits
sdkconfig defaults, board overlays, or build profiles.
EOF
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

warn_if_non_target_dir() {
  local dir="$1"
  case "$dir" in
    "$REPO_ROOT"/target|"$REPO_ROOT"/target/*|target|target/*)
      ;;
    *)
      echo "Warning: evidence dir is outside target/: $dir" >&2
      echo "         Generated evidence may be picked up by git status; do not commit run artifacts." >&2
      ;;
  esac
}

profile=""
artifact=""
log_file=""
operator="${USER:-unknown}"
board="esp32s3"
evidence_dir="$REPO_ROOT/target/esp-profile-matrix"
heap_largest_floor="${HEAP_LARGEST_FLOOR:-32768}"
init_only=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --profile requires a value." >&2; exit 2; }
      profile="$1"
      ;;
    --artifact)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --artifact requires a value." >&2; exit 2; }
      artifact="$1"
      ;;
    --log)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --log requires a value." >&2; exit 2; }
      log_file="$1"
      ;;
    --operator)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --operator requires a value." >&2; exit 2; }
      operator="$1"
      ;;
    --board)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --board requires a value." >&2; exit 2; }
      board="$1"
      ;;
    --evidence-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --evidence-dir requires a value." >&2; exit 2; }
      evidence_dir="$1"
      ;;
    --heap-largest-floor)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --heap-largest-floor requires a value." >&2; exit 2; }
      heap_largest_floor="$1"
      ;;
    --init-only)
      init_only=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "Error: unknown argument: $1" >&2
      usage
      exit 2
      ;;
    *)
      echo "Error: unexpected positional argument: $1" >&2
      usage
      exit 2
      ;;
  esac
  shift
done

case "$profile" in
  ""|baseline|alwaysinternal_0|alwaysinternal_1024|alwaysinternal_2048|xip_psram_candidate)
    ;;
  *)
    echo "Error: unsupported profile row: $profile" >&2
    exit 2
    ;;
esac
if [[ -n "$log_file" && ! -f "$log_file" ]]; then
  echo "Error: log file not found: $log_file" >&2
  exit 1
fi
[[ "$heap_largest_floor" =~ ^[0-9]+$ ]] || {
  echo "Error: --heap-largest-floor must be an integer byte count." >&2
  exit 2
}
warn_if_non_target_dir "$evidence_dir"

mkdir -p "$evidence_dir"
matrix_csv="$evidence_dir/matrix.csv"
commands_md="$evidence_dir/commands.md"
readme_md="$evidence_dir/README.md"

if [[ ! -f "$matrix_csv" ]]; then
  echo "created_utc,profile,board,operator,artifact,log,analysis_dir,boot_resource_baseline,heartbeat_180s_trend,config_ui_smoke,wss_connect_smoke,display_smoke,voice_path_smoke,panic_coredump_absence,notes" > "$matrix_csv"
fi

cat > "$commands_md" <<'EOF'
# ESP profile matrix commands

These commands are intentionally printed for a human operator. This script does
not edit `sdkconfig.defaults.*`, board overlays, or any default profile.

## baseline

```bash
TARGET=esp ./build.sh --no-deploy
bash scripts/esp_soak_collect.sh --port /dev/tty.usbserial-XXXX --duration 180 --scenario baseline --analyze
bash scripts/esp_profile_matrix.sh --profile baseline --artifact target/esp-artifacts/<artifact-id> --log target/esp-soak/<run>/serial.log
```

## alwaysinternal_0

Apply Candidate A in a throwaway worktree or local experiment branch:

```ini
CONFIG_SPIRAM_MALLOC_ALWAYSINTERNAL=0
CONFIG_SPIRAM_MALLOC_RESERVE_INTERNAL=98304
```

Do not enable external task stacks for this candidate, then run:

```bash
TARGET=esp ./build.sh --no-deploy
bash scripts/esp_soak_collect.sh --port /dev/tty.usbserial-XXXX --duration 180 --scenario alwaysinternal_0 --analyze
bash scripts/esp_profile_matrix.sh --profile alwaysinternal_0 --artifact target/esp-artifacts/<artifact-id> --log target/esp-soak/<run>/serial.log
```

## xip_psram_candidate

Apply Candidate B only on boards that support PSRAM XIP and have stable boot logs:

```ini
CONFIG_SPIRAM_XIP_FROM_PSRAM=y
```

Then run the same build, soak collection, and profile-matrix record commands with
`--profile xip_psram_candidate`.

## alwaysinternal_1024

Apply Candidate C (esp-box style small-object threshold, keeping Beetle reserve):

```ini
CONFIG_SPIRAM_MALLOC_ALWAYSINTERNAL=1024
CONFIG_SPIRAM_MALLOC_RESERVE_INTERNAL=98304
CONFIG_MBEDTLS_EXTERNAL_MEM_ALLOC=y
CONFIG_MBEDTLS_DYNAMIC_BUFFER=y
CONFIG_SPIRAM_TRY_ALLOCATE_WIFI_LWIP=y
```

Do not change task stacks, DMA descriptors, Wi-Fi/NVS core structures, or TLS
stack family. Then run the same build, soak collection, and profile-matrix
record commands with `--profile alwaysinternal_1024`.

## alwaysinternal_2048

Apply Candidate D (smaller internal-allocation threshold, keeping Beetle reserve):

```ini
CONFIG_SPIRAM_MALLOC_ALWAYSINTERNAL=2048
CONFIG_SPIRAM_MALLOC_RESERVE_INTERNAL=98304
```

Then run the same build, soak collection, and profile-matrix record commands with
`--profile alwaysinternal_2048`.
EOF

cat > "$readme_md" <<EOF
# ESP Profile Matrix Evidence

This directory is generated evidence under \`target/\`. It is not a source of
defaults. Candidate profile rows must be applied manually outside this script.

Rows:

- baseline
- alwaysinternal_0
- alwaysinternal_1024
- alwaysinternal_2048
- xip_psram_candidate

Decision rule: a candidate can only move toward a default change after the
recorded artifact has no new warning/error, the largest internal block trend is
better or stays above floor, config/WSS/display/voice smokes do not regress, and
rollback to baseline is a single-profile change.
EOF

if [[ "$init_only" -eq 1 || -z "$profile" ]]; then
  echo "ESP profile matrix initialized:"
  echo "  evidence dir: $evidence_dir"
  echo "  matrix: $matrix_csv"
  echo "  commands: $commands_md"
  echo "  readme: $readme_md"
  exit 0
fi

run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$-$profile"
run_dir="$evidence_dir/$run_id"
analysis_dir=""
mkdir -p "$run_dir"

if [[ -n "$log_file" ]]; then
  cp "$log_file" "$run_dir/serial.log"
  analysis_root="$run_dir/analysis"
  bash "$SCRIPT_DIR/esp_soak_analyze.sh" --output-dir "$analysis_root" --heap-largest-floor "$heap_largest_floor" "$run_dir/serial.log"
  analysis_dir="$(find "$analysis_root" -mindepth 1 -maxdepth 1 -type d | sort | tail -n 1)"
fi

{
  echo "run_id=$run_id"
  echo "profile=$profile"
  echo "board=$board"
  echo "operator=$operator"
  echo "artifact=$artifact"
  echo "log=$log_file"
  echo "analysis_dir=$analysis_dir"
  echo "created_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$run_dir/metadata.env"

created_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n' \
  "$created_utc" \
  "$profile" \
  "$board" \
  "$operator" \
  "$artifact" \
  "$log_file" \
  "$analysis_dir" \
  "operator-recorded" \
  "see-analysis" \
  "manual-required" \
  "manual-required" \
  "manual-required" \
  "manual-if-hardware" \
  "see-analysis" \
  "evidence-only-no-default-change" >> "$matrix_csv"

echo "ESP profile matrix row recorded:"
echo "  evidence dir: $evidence_dir"
echo "  run dir: $run_dir"
echo "  matrix: $matrix_csv"
echo "  commands: $commands_md"

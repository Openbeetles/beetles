#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/esp_soak_collect.sh [options]

Options:
  --input FILE          Copy an existing captured log into a new evidence run.
  --port DEVICE         Serial device to read, for example /dev/tty.usbserial-*.
  --baud BAUD           Serial baud rate. Default: 115200.
  --duration SECONDS    Capture duration. Default: 180. Use 0 for manual Ctrl-C.
  --scenario NAME       Scenario label. Default: boot_idle.
  --output-dir DIR      Evidence root. Default: target/esp-soak.
  --analyze             Run scripts/esp_soak_analyze.sh after capture/copy.

This script writes local evidence only. It does not use the network.
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
      echo "Warning: output dir is outside target/: $dir" >&2
      echo "         Generated evidence may be picked up by git status; do not commit run artifacts." >&2
      ;;
  esac
}

input_file=""
port=""
baud="115200"
duration="180"
scenario="boot_idle"
output_dir="$REPO_ROOT/target/esp-soak"
run_analyze=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --input)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --input requires a value." >&2; exit 2; }
      input_file="$1"
      ;;
    --port)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --port requires a value." >&2; exit 2; }
      port="$1"
      ;;
    --baud)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --baud requires a value." >&2; exit 2; }
      baud="$1"
      ;;
    --duration)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --duration requires a value." >&2; exit 2; }
      duration="$1"
      ;;
    --scenario)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --scenario requires a value." >&2; exit 2; }
      scenario="$1"
      ;;
    --output-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --output-dir requires a value." >&2; exit 2; }
      output_dir="$1"
      ;;
    --analyze)
      run_analyze=1
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

if [[ -n "$input_file" && -n "$port" ]]; then
  echo "Error: choose either --input or --port, not both." >&2
  exit 2
fi
if [[ -z "$input_file" && -z "$port" ]]; then
  usage
  exit 2
fi
if [[ -n "$input_file" && ! -f "$input_file" ]]; then
  echo "Error: input file not found: $input_file" >&2
  exit 1
fi
if [[ -n "$port" && ! -e "$port" ]]; then
  echo "Error: serial device not found: $port" >&2
  exit 1
fi
[[ "$duration" =~ ^[0-9]+$ ]] || { echo "Error: --duration must be an integer." >&2; exit 2; }
warn_if_non_target_dir "$output_dir"

run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$-$scenario"
run_dir="$output_dir/$run_id"
log_file="$run_dir/serial.log"
mkdir -p "$run_dir"

{
  echo "run_id=$run_id"
  echo "scenario=$scenario"
  echo "baud=$baud"
  echo "duration_seconds=$duration"
  echo "input_file=$input_file"
  echo "port=$port"
  echo "created_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$run_dir/metadata.env"

if [[ -n "$input_file" ]]; then
  cp "$input_file" "$log_file"
else
  if stty -f "$port" "$baud" raw -echo 2>/dev/null; then
    :
  elif stty -F "$port" "$baud" raw -echo 2>/dev/null; then
    :
  else
    echo "Error: failed to configure serial device with stty: $port" >&2
    exit 1
  fi

  echo "Capturing serial log to $log_file" >&2
  if [[ "$duration" -eq 0 ]]; then
    cat "$port" > "$log_file"
  else
    cat "$port" > "$log_file" &
    cat_pid="$!"
    sleep "$duration"
    kill "$cat_pid" 2>/dev/null || true
    wait "$cat_pid" 2>/dev/null || true
  fi
fi

{
  echo "# Reproduce ESP soak collection"
  echo
  if [[ -n "$input_file" ]]; then
    printf 'scripts/esp_soak_collect.sh --input %q --scenario %q --output-dir %q\n' "$input_file" "$scenario" "$output_dir"
  else
    printf 'scripts/esp_soak_collect.sh --port %q --baud %q --duration %q --scenario %q --output-dir %q\n' "$port" "$baud" "$duration" "$scenario" "$output_dir"
  fi
  echo
  printf 'scripts/esp_soak_analyze.sh --output-dir %q %q\n' "$run_dir/analysis" "$log_file"
} > "$run_dir/commands.md"

if [[ "$run_analyze" -eq 1 ]]; then
  bash "$SCRIPT_DIR/esp_soak_analyze.sh" --output-dir "$run_dir/analysis" "$log_file"
fi

echo "ESP soak evidence written:"
echo "  run dir: $run_dir"
echo "  log: $log_file"
echo "  metadata: $run_dir/metadata.env"
echo "  commands: $run_dir/commands.md"

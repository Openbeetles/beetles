#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage:
  scripts/parse_esp_panic_log.sh [--artifact-dir DIR] <serial-log>

Options:
  --artifact-dir DIR   Artifact directory for the suggested symbolization command.
                       Defaults to target/esp-artifacts/<artifact-id> when the
                       log contains artifact_id=..., otherwise a placeholder.

Output:
  A local, network-free evidence summary with panic core, reason, PC,
  backtrace PC addresses, ELF SHA when the captured log contains one, and the
  suggested scripts/esp_symbolize_panic.sh command.

Example:
  scripts/parse_esp_panic_log.sh \
    --artifact-dir target/esp-artifacts/<artifact-id> \
    target/esp-soak/<run>/serial.log
EOF
}

artifact_dir=""
log_file=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --artifact-dir)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --artifact-dir requires a value." >&2; exit 2; }
      artifact_dir="$1"
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
      [[ -z "$log_file" ]] || { echo "Error: only one serial log is supported per run." >&2; exit 2; }
      log_file="$1"
      ;;
  esac
  shift
done

[[ -n "$log_file" ]] || { usage; exit 2; }
[[ -f "$log_file" ]] || { echo "Error: serial log not found: $log_file" >&2; exit 1; }

awk -v log_file="$log_file" -v artifact_dir_arg="$artifact_dir" '
function trim(value) {
  gsub(/^[[:space:]]+|[[:space:]]+$/, "", value);
  return value;
}

function append_unique(list, seen, addr,    result) {
  if (addr == "" || seen[addr]) {
    return list;
  }
  seen[addr] = 1;
  result = list;
  if (result != "") {
    result = result " ";
  }
  return result addr;
}

function capture_pc(line,    token) {
  if (pc == "" && match(line, /PC[[:space:]]*:[[:space:]]*0x[[:xdigit:]]+/)) {
    token = substr(line, RSTART, RLENGTH);
    sub(/^.*0x/, "0x", token);
    pc = token;
  }
  if (pc == "" && match(line, /PC[[:space:]]+0x[[:xdigit:]]+/)) {
    token = substr(line, RSTART, RLENGTH);
    sub(/^.*0x/, "0x", token);
    pc = token;
  }
}

function capture_backtrace(line,    rest, count, parts, i, token, addr) {
  rest = line;
  sub(/^.*Backtrace:[[:space:]]*/, "", rest);
  count = split(rest, parts, /[[:space:]]+/);
  for (i = 1; i <= count; i++) {
    token = parts[i];
    if (match(token, /0x[[:xdigit:]]+(:0x[[:xdigit:]]+)?/)) {
      addr = substr(token, RSTART, RLENGTH);
      sub(/:.*/, "", addr);
      backtrace_addresses = append_unique(backtrace_addresses, backtrace_seen, addr);
      symbol_addresses = append_unique(symbol_addresses, symbol_seen, addr);
    }
  }
}

function capture_elf_sha(line,    lower, rest, sha) {
  lower = tolower(line);
  if (lower !~ /elf/ || lower !~ /sha/) {
    return;
  }
  rest = line;
  while (match(rest, /[0-9a-fA-F]{8,64}/)) {
    sha = substr(rest, RSTART, RLENGTH);
    if (length(sha) >= 8) {
      elf_sha = sha;
    }
    rest = substr(rest, RSTART + RLENGTH);
  }
}

function capture_artifact_id(line,    value) {
  if (artifact_id == "" && line ~ /artifact_id=/) {
    value = line;
    sub(/^.*artifact_id=/, "", value);
    sub(/[[:space:]].*$/, "", value);
    artifact_id = trim(value);
  }
  if (artifact_id == "" && line ~ /Artifact id:/) {
    value = line;
    sub(/^.*Artifact id:[[:space:]]*/, "", value);
    sub(/[[:space:]].*$/, "", value);
    artifact_id = trim(value);
  }
}

function capture_reason(line,    lower, value) {
  lower = tolower(line);
  if (panic_line == 0 && (line ~ /Guru Meditation/ || line ~ /Core[[:space:]]+[0-9]+ panic/ || lower ~ /panic.ed|core dump|coredump.*(written|stored|checksum|panic)|stack overflow in task|assert failed|abort\(\) was called/)) {
    panic_line = NR;
  }
  if (core == "" && match(line, /Core[[:space:]]+[0-9]+/)) {
    value = substr(line, RSTART, RLENGTH);
    gsub(/[^0-9]/, "", value);
    core = value;
  }
  if (core == "" && match(line, /CPU[0-9]+/)) {
    value = substr(line, RSTART, RLENGTH);
    gsub(/[^0-9]/, "", value);
    core = value;
  }
  if (reason == "" && line ~ /Guru Meditation/ && match(line, /\([^)]*\)/)) {
    value = substr(line, RSTART + 1, RLENGTH - 2);
    reason = trim(value);
  }
  if (reason == "" && lower ~ /interrupt wdt timeout/) {
    reason = "Interrupt wdt timeout";
  }
  if (reason == "" && lower ~ /stack overflow/) {
    reason = trim(line);
  }
  if (reason == "" && lower ~ /assert failed/) {
    reason = trim(line);
  }
  if (reason == "" && lower ~ /abort\(\) was called/) {
    reason = "abort() was called";
  }
  if (reason == "" && lower ~ /^panic[: ]/) {
    reason = trim(line);
  }
}

BEGIN {
  pc = "";
  core = "";
  reason = "";
  elf_sha = "";
  artifact_id = "";
  backtrace_addresses = "";
  symbol_addresses = "";
  last_resource_baseline = "";
  panic_line = 0;
}

{
  line = $0;
  lower = tolower(line);
  if (panic_line == 0 && (lower ~ /resource baseline/ || lower ~ /resource_baseline/ || lower ~ /orchestrator.*baseline/)) {
    last_resource_baseline = trim(line);
  }
  capture_artifact_id(line);
  capture_elf_sha(line);
  capture_reason(line);
  if (panic_line != 0) {
    capture_pc(line);
    if (line ~ /Backtrace:/) {
      capture_backtrace(line);
    } else if (in_backtrace && line ~ /0x[[:xdigit:]]+:/) {
      capture_backtrace("Backtrace: " line);
    }
    if (line ~ /Backtrace:/) {
      in_backtrace = 1;
    } else if (in_backtrace && trim(line) == "") {
      in_backtrace = 0;
    }
  }
}

END {
  if (panic_line == 0) {
    last_resource_baseline = "";
  }
  symbol_addresses = append_unique(symbol_addresses, symbol_seen, pc);
  if (symbol_addresses == "") {
    symbol_addresses = "<addresses...>";
  }

  if (artifact_dir_arg != "") {
    artifact_dir = artifact_dir_arg;
  } else if (artifact_id != "") {
    artifact_dir = "target/esp-artifacts/" artifact_id;
  } else {
    artifact_dir = "target/esp-artifacts/<artifact-id>";
  }

  print "log_file: " log_file;
  print "panic_line: " panic_line;
  print "panic_core: " core;
  print "panic_reason: " reason;
  print "panic_pc: " pc;
  print "backtrace_addresses: " backtrace_addresses;
  print "elf_sha256: " elf_sha;
  print "artifact_id: " artifact_id;
  print "last_resource_baseline_before_panic: " last_resource_baseline;
  print "suggested_symbolization_command: scripts/esp_symbolize_panic.sh " artifact_dir " " symbol_addresses;
}
' "$log_file"

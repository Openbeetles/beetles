#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
MAIN_RS="$ROOT_DIR/src/main.rs"

platform_init_line=$(rg -n 'let platform: Arc<dyn Platform> = Arc::new\(LinuxPlatform::new\(\)\);' "$MAIN_RS" | head -n1 | cut -d: -f1 || true)
fastpath_line=$(rg -n 'match &cli\.command' "$MAIN_RS" | head -n1 | cut -d: -f1 || true)

if [[ -z "$platform_init_line" || -z "$fastpath_line" ]]; then
  echo "FAIL: expected Linux main fast-path and platform init markers in src/main.rs" >&2
  exit 1
fi

if (( fastpath_line >= platform_init_line )); then
  echo "FAIL: Linux CLI fast-path must run before platform initialization" >&2
  echo "  fast-path line: $fastpath_line" >&2
  echo "  platform init line: $platform_init_line" >&2
  exit 1
fi

for pattern in \
  'Commands::Restart =>' \
  'handle_restart_command();' \
  'Commands::Stop =>' \
  'handle_stop_command();' \
  'Commands::Version =>' \
  'Commands::ReasoningRunner =>'; do
  if ! rg -F -n "$pattern" "$MAIN_RS" >/dev/null 2>&1; then
    echo "FAIL: missing fast-path pattern in src/main.rs: $pattern" >&2
    exit 1
  fi
done

echo "linux_cli_fastpath_test: ok"

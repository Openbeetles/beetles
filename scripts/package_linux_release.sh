#!/usr/bin/env bash
# Build one or more beetle-${VERSION}-linux-<arch>.tar.gz bundles from Linux build artifacts
# plus packaging/linux templates.
# Usage:
#   ./scripts/package_linux_release.sh --binary target/<triple>/release/beetle --target <triple>
#   ./scripts/package_linux_release.sh --version v0.1.0 --armv7 path/to/beetle [--aarch64 path/to/beetle] [--x86_64 path/to/beetle] [--riscv64 path/to/beetle] [--output-dir dist]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION=""
TARGET_TRIPLE=""
BINARY_PATH=""
X86_64_BIN=""
ARMV7_BIN=""
AARCH64_BIN=""
RISCV64_BIN=""
OUTPUT_DIR="$REPO_ROOT/dist"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/package_linux_release.sh --binary target/<triple>/release/beetle --target <triple> [--version vX.Y.Z] [--output-dir dist]
  ./scripts/package_linux_release.sh [--version vX.Y.Z] [--x86_64 PATH] [--armv7 PATH] [--aarch64 PATH] [--riscv64 PATH] [--output-dir DIR]

Notes:
  - When --version is omitted, the script reads package.version from Cargo.toml and uses v<version>.
  - The --binary/--target form is the simple single-artifact mode used by build.sh.
  - The per-arch form remains available for multi-artifact release assembly.
EOF
}

infer_version_from_cargo() {
  local detected=""
  detected="$(
    awk '
      /^\[package\]$/ { in_package=1; next }
      /^\[/ { if (in_package) exit }
      in_package && /^version[[:space:]]*=/ {
        match($0, /"[^"]+"/)
        if (RSTART > 0) {
          print substr($0, RSTART + 1, RLENGTH - 2)
          exit
        }
      }
    ' "$REPO_ROOT/Cargo.toml"
  )"
  [[ -n "$detected" ]] || {
    echo "Unable to infer package version from $REPO_ROOT/Cargo.toml" >&2
    exit 1
  }
  printf 'v%s\n' "$detected"
}

target_label_from_triple() {
  case "${1:-}" in
    x86_64-unknown-linux-*) printf '%s\n' "x86_64" ;;
    armv7-unknown-linux-*) printf '%s\n' "armv7" ;;
    aarch64-unknown-linux-*) printf '%s\n' "aarch64" ;;
    riscv64-unknown-linux-*) printf '%s\n' "riscv64" ;;
    *)
      echo "Unsupported Linux target triple for packaging: ${1:-<empty>}" >&2
      exit 1
      ;;
  esac
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BINARY_PATH="${2:-}"; shift 2 ;;
    --target) TARGET_TRIPLE="${2:-}"; shift 2 ;;
    --x86_64) X86_64_BIN="${2:-}"; shift 2 ;;
    --version) VERSION="${2:-}"; shift 2 ;;
    --armv7) ARMV7_BIN="${2:-}"; shift 2 ;;
    --aarch64) AARCH64_BIN="${2:-}"; shift 2 ;;
    --riscv64) RISCV64_BIN="${2:-}"; shift 2 ;;
    --output-dir) OUTPUT_DIR="${2:-}"; shift 2 ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  VERSION="$(infer_version_from_cargo)"
fi
if [[ ! "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "VERSION must look like v0.1.0, got: $VERSION" >&2
  exit 1
fi

single_artifact_mode=0
if [[ -n "$BINARY_PATH" || -n "$TARGET_TRIPLE" ]]; then
  single_artifact_mode=1
fi

if [[ "$single_artifact_mode" -eq 1 && ( -n "$X86_64_BIN" || -n "$ARMV7_BIN" || -n "$AARCH64_BIN" || -n "$RISCV64_BIN" ) ]]; then
  echo "Do not mix --binary/--target with per-arch release arguments." >&2
  exit 1
fi

if [[ "$single_artifact_mode" -eq 1 ]]; then
  if [[ -z "$BINARY_PATH" || -z "$TARGET_TRIPLE" ]]; then
    echo "Single-artifact mode requires both --binary and --target." >&2
    exit 1
  fi
  case "$(target_label_from_triple "$TARGET_TRIPLE")" in
    x86_64) X86_64_BIN="$BINARY_PATH" ;;
    armv7) ARMV7_BIN="$BINARY_PATH" ;;
    aarch64) AARCH64_BIN="$BINARY_PATH" ;;
    riscv64) RISCV64_BIN="$BINARY_PATH" ;;
  esac
fi

if [[ -z "$X86_64_BIN" && -z "$ARMV7_BIN" && -z "$AARCH64_BIN" && -z "$RISCV64_BIN" ]]; then
  echo "Provide at least one Linux artifact: --binary/--target or one of --x86_64/--armv7/--aarch64/--riscv64" >&2
  exit 1
fi

PKG_ROOT="$REPO_ROOT/packaging/linux"
for f in beetle.service beetle.init beetle.env.example hardware.json.example README.txt; do
  if [[ ! -f "$PKG_ROOT/$f" ]]; then
    echo "Missing packaging template: $PKG_ROOT/$f" >&2
    exit 1
  fi
done

mkdir -p "$OUTPUT_DIR"

make_tarball() {
  local triple="$1"
  local bin_path="$2"
  local name="beetle-${VERSION}-linux-${triple}"
  local stag_dir
  stag_dir="$(mktemp -d "${TMPDIR:-/tmp}/beetle-pkg-${triple}.XXXXXX")"
  mkdir -p "$stag_dir/$name"
  cp "$bin_path" "$stag_dir/$name/beetle"
  chmod 755 "$stag_dir/$name/beetle"
  cp "$PKG_ROOT/beetle.service" "$PKG_ROOT/beetle.init" "$PKG_ROOT/beetle.env.example" "$PKG_ROOT/hardware.json.example" "$PKG_ROOT/README.txt" "$stag_dir/$name/"
  chmod 755 "$stag_dir/$name/beetle.init"
  local out="$OUTPUT_DIR/${name}.tar.gz"
  (cd "$stag_dir" && tar -czf "$out" "$name")
  rm -rf "$stag_dir"
  echo "Wrote $out"
}

package_if_present() {
  local triple="$1"
  local bin_path="$2"

  if [[ -z "$bin_path" ]]; then
    return 0
  fi
  if [[ ! -f "$bin_path" ]]; then
    echo "Binary not found for ${triple}: $bin_path" >&2
    exit 1
  fi

  make_tarball "$triple" "$bin_path"
}

package_if_present x86_64 "$X86_64_BIN"
package_if_present armv7 "$ARMV7_BIN"
package_if_present aarch64 "$AARCH64_BIN"
package_if_present riscv64 "$RISCV64_BIN"

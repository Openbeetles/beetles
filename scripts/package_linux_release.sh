#!/usr/bin/env bash
# Build one or more beetle-${VERSION}-linux-<arch>.tar.gz bundles from cross-built binaries
# plus packaging/linux templates.
# Usage:
#   ./scripts/package_linux_release.sh --version v0.1.0 --armv7 path/to/beetle [--aarch64 path/to/beetle] [--riscv64 path/to/beetle] [--output-dir dist]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION=""
ARMV7_BIN=""
AARCH64_BIN=""
RISCV64_BIN=""
OUTPUT_DIR="$REPO_ROOT/dist"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="${2:-}"; shift 2 ;;
    --armv7) ARMV7_BIN="${2:-}"; shift 2 ;;
    --aarch64) AARCH64_BIN="${2:-}"; shift 2 ;;
    --riscv64) RISCV64_BIN="${2:-}"; shift 2 ;;
    --output-dir) OUTPUT_DIR="${2:-}"; shift 2 ;;
    -h|--help)
      echo "Usage: $0 --version vX.Y.Z [--armv7 PATH] [--aarch64 PATH] [--riscv64 PATH] [--output-dir DIR]"
      exit 0
      ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  echo "Required: --version" >&2
  exit 1
fi
if [[ -z "$ARMV7_BIN" && -z "$AARCH64_BIN" && -z "$RISCV64_BIN" ]]; then
  echo "Provide at least one binary: --armv7, --aarch64, or --riscv64" >&2
  exit 1
fi
if [[ ! "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "VERSION must look like v0.1.0, got: $VERSION" >&2
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

package_if_present armv7 "$ARMV7_BIN"
package_if_present aarch64 "$AARCH64_BIN"
package_if_present riscv64 "$RISCV64_BIN"

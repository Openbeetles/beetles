#!/usr/bin/env bash
# Vendor espressif/esp-dsp v1.7.1 for IDF 6 + gnu++26 (registry 1.7.0 EKF uses std::cos and fails with picolibc).
# Run from repo root: ./scripts/fetch-esp-dsp-vendor.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DST="${ROOT}/third_party/espressif__esp-dsp"
if [[ -f "${DST}/CMakeLists.txt" ]]; then
  echo "esp-dsp vendor already present: ${DST}"
  exit 0
fi
mkdir -p "${ROOT}/third_party"
# Tag v1.7.1 may not resolve as a commit with --depth 1 on some git versions; fall back to known good tree.
if ! git clone --depth 1 --branch v1.7.1 https://github.com/espressif/esp-dsp.git "${DST}" 2>/dev/null; then
  rm -rf "${DST}"
  git clone --depth 80 https://github.com/espressif/esp-dsp.git "${DST}"
  git -C "${DST}" checkout fd110921fb57b4de79f092337b9e047130762eca
fi
echo "OK: ${DST}"

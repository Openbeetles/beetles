#!/usr/bin/env bash
# Sync local Luckfox Lyra SDK tree into the Docker volume used as /work in the build container.
# Uses rsync (via a short-lived Alpine container) because `docker cp` breaks on large .repo packs / odd symlinks.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="${LUCKFOX_SDK_PATH:-$ROOT/linux/Luckfox_Lyra_SDK_250815}"
VOL="${LUCKFOX_DOCKER_VOLUME:-luckfox-sdk-work}"
CONTAINER="${LUCKFOX_DOCKER_CONTAINER:-ubuntu2204}"

if ! test -d "$SDK"; then
  echo "error: SDK directory not found: $SDK" >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "error: Docker is not reachable" >&2
  exit 1
fi

if ! docker volume inspect "$VOL" >/dev/null 2>&1; then
  echo "error: Docker volume not found: $VOL (create it or set LUCKFOX_DOCKER_VOLUME)" >&2
  exit 1
fi

docker start "$CONTAINER" >/dev/null 2>&1 || true

echo "Syncing (rsync) from:"
echo "  $SDK"
echo "-> volume ${VOL}:/work"
echo "Size (local): $(du -sh "$SDK" | cut -f1)"

docker run --rm \
  -v "${VOL}:/work" \
  -v "${SDK}:/src:ro" \
  alpine:3.20 \
  sh -c 'apk add --no-cache rsync >/dev/null && rsync -a /src/ /work/'

echo "Done. Container ${CONTAINER} uses this volume as /work — no second copy step."

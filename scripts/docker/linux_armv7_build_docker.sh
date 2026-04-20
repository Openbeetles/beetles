#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

CONTAINER_NAME="orangepi-zero-lts-bionic"
IMAGE="orangepi-zero-lts:bionic-2.0.8"
WORKDIR_IN_CONTAINER="/workspace/beetle"
BUILD_TARGET="armv7-unknown-linux-gnueabihf"

print_step() {
  printf '\n==> %s\n' "$1"
}

print_done() {
  printf '    %s\n' "$1"
}

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "error: missing required command: $command_name" >&2
    exit 1
  fi
}

require_docker_daemon() {
  if ! docker info >/dev/null 2>&1; then
    echo "error: Docker is not reachable. Start Docker Desktop and try again." >&2
    exit 1
  fi
}

container_exists() {
  docker container inspect "$CONTAINER_NAME" >/dev/null 2>&1
}

image_exists() {
  docker image inspect "$IMAGE" >/dev/null 2>&1
}

remove_existing_container() {
  if container_exists; then
    print_step "Removing existing container $CONTAINER_NAME"
    docker rm -f "$CONTAINER_NAME" >/dev/null
    print_done "Removed old container"
  fi
}

create_container() {
  if ! image_exists; then
    echo "error: required image $IMAGE is not available locally." >&2
    echo "Build/import the dedicated Orange Pi armv7 image first, then re-run this script." >&2
    exit 1
  fi

  print_step "Creating container $CONTAINER_NAME"
  docker create \
    --name "$CONTAINER_NAME" \
    --platform linux/arm/v7 \
    -v "$ROOT_DIR:/workspace/beetle" \
    -w "$WORKDIR_IN_CONTAINER" \
    "$IMAGE" \
    sleep infinity >/dev/null
  print_done "Container created"
}

start_container() {
  print_step "Starting container $CONTAINER_NAME"
  docker start "$CONTAINER_NAME" >/dev/null
  print_done "Container started"
}

verify_environment() {
  print_step "Verifying armv7 GNU build environment"
  docker exec "$CONTAINER_NAME" /bin/bash -lc "uname -a | sed -n '1p'"
  docker exec "$CONTAINER_NAME" /bin/bash -lc "cd $WORKDIR_IN_CONTAINER && cargo +stable -V"
  docker exec "$CONTAINER_NAME" /bin/bash -lc "pkg-config --modversion alsa"
  docker exec "$CONTAINER_NAME" /bin/bash -lc "pkg-config --modversion libudev"
  print_done "GNU target to use: $BUILD_TARGET"
}

show_next_steps() {
  cat <<EOF

Linux armv7 GNU build container is ready.

Container:
  $CONTAINER_NAME

Repository mount:
  $ROOT_DIR -> $WORKDIR_IN_CONTAINER

Enter the container:
  docker exec -it $CONTAINER_NAME /bin/bash

Build Beetle for Linux armv7 GNU:
  docker exec -it $CONTAINER_NAME /bin/bash -lc 'cd $WORKDIR_IN_CONTAINER && cargo +stable build --release --target armv7-unknown-linux-gnueabihf --no-default-features --features default_runtime,capability_voice,capability_vision,capability_sensor,capability_office'

Inside the container, Beetle lives at:
  $WORKDIR_IN_CONTAINER
EOF
}

main() {
  require_command docker
  require_docker_daemon
  remove_existing_container
  create_container
  start_container
  verify_environment
  show_next_steps
}

main "$@"

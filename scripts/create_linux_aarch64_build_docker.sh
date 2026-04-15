#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

CONTAINER_NAME="beetle-linux-aarch64"
IMAGE="rust:1-bookworm"
WORKDIR_IN_CONTAINER="/workspace"
BUILD_TARGET="aarch64-unknown-linux-gnu"
APT_PACKAGES=(
  build-essential
  pkg-config
  libasound2-dev
  libudev-dev
  ca-certificates
  curl
  wget
  git
)

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

remove_existing_container() {
  if docker container inspect "$CONTAINER_NAME" >/dev/null 2>&1; then
    print_step "Removing existing container $CONTAINER_NAME"
    docker rm -f "$CONTAINER_NAME" >/dev/null
    print_done "Removed old container"
  fi
}

create_container() {
  print_step "Pulling build image $IMAGE"
  docker pull "$IMAGE" >/dev/null
  print_done "Image ready"

  print_step "Creating container $CONTAINER_NAME"
  docker create \
    --name "$CONTAINER_NAME" \
    --platform linux/arm64 \
    -e RUSTUP_TOOLCHAIN=stable \
    -v "$ROOT_DIR:/workspace" \
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

bootstrap_packages() {
  print_step "Installing Linux build dependencies"
  docker exec "$CONTAINER_NAME" bash -c \
    'apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential pkg-config libasound2-dev libudev-dev ca-certificates curl wget git' >/dev/null
  print_done "System packages installed"
}

verify_environment() {
  print_step "Verifying Rust and target environment"
  docker exec "$CONTAINER_NAME" bash -c "rustc -vV | sed -n '1,6p'"
  docker exec "$CONTAINER_NAME" bash -c "cargo -V"
  print_done "GNU target to use: $BUILD_TARGET"
}

show_next_steps() {
  cat <<EOF

Linux aarch64 build container is ready.

Container:
  $CONTAINER_NAME

Repository mount:
  $ROOT_DIR -> $WORKDIR_IN_CONTAINER

Enter the container:
  docker exec -it $CONTAINER_NAME bash

Build Beetle for Linux aarch64 GNU:
  docker exec -it $CONTAINER_NAME bash -c 'cd $WORKDIR_IN_CONTAINER && cargo build --release --target aarch64-unknown-linux-gnu'

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
  bootstrap_packages
  verify_environment
  show_next_steps
}

main "$@"

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

CONTAINER_NAME="beetle-linux-aarch64"
IMAGE="rust:1-bookworm"
WORKDIR_IN_CONTAINER="/workspace/beetle"
BUILD_TARGET="aarch64-unknown-linux-gnu"
TARGET_LINKER="aarch64-linux-gnu-gcc"
TARGET_AR="aarch64-linux-gnu-ar"
TARGET_PKG_CONFIG_LIBDIR="/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig"
CONTAINER_PATH="/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
NATIVE_APT_PACKAGES=(
  build-essential
  pkg-config
  libasound2-dev
  libudev-dev
  ca-certificates
  curl
  wget
  git
)
CROSS_APT_PACKAGES=(
  build-essential
  pkg-config
  gcc-aarch64-linux-gnu
  g++-aarch64-linux-gnu
  libc6-dev-arm64-cross
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

host_docker_platform() {
  case "$(uname -m)" in
    arm64|aarch64)
      printf '%s\n' "linux/arm64"
      ;;
    x86_64|amd64)
      printf '%s\n' "linux/amd64"
      ;;
    *)
      echo "error: unsupported host architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac
}

DOCKER_PLATFORM="$(host_docker_platform)"

uses_cross_toolchain() {
  [ "$DOCKER_PLATFORM" != "linux/arm64" ]
}

container_exists() {
  docker container inspect "$CONTAINER_NAME" >/dev/null 2>&1
}

image_exists() {
  docker image inspect "$IMAGE" >/dev/null 2>&1
}

container_needs_recreate() {
  if ! container_exists; then
    return 0
  fi

  local mount_source=""
  mount_source="$(docker inspect "$CONTAINER_NAME" --format '{{range .Mounts}}{{if eq .Destination "/workspace/beetle"}}{{.Source}}{{end}}{{end}}' 2>/dev/null || true)"
  if [ "$mount_source" != "$ROOT_DIR" ]; then
    return 0
  fi

  local image=""
  image="$(docker inspect "$CONTAINER_NAME" --format '{{.Config.Image}}' 2>/dev/null || true)"
  if [ "$image" != "$IMAGE" ]; then
    return 0
  fi

  local platform=""
  platform="$(docker inspect "$CONTAINER_NAME" --format '{{.HostConfig.Platform}}' 2>/dev/null || true)"
  if [ "$platform" != "$DOCKER_PLATFORM" ]; then
    return 0
  fi

  local env_dump=""
  env_dump="$(docker inspect "$CONTAINER_NAME" --format '{{range .Config.Env}}{{println .}}{{end}}' 2>/dev/null || true)"
  if ! printf '%s\n' "$env_dump" | grep -qx "PATH=$CONTAINER_PATH"; then
    return 0
  fi

  if uses_cross_toolchain; then
    if ! printf '%s\n' "$env_dump" | grep -qx "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$TARGET_LINKER"; then
      return 0
    fi
  fi

  return 1
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
    print_step "Pulling build image $IMAGE"
    docker pull "$IMAGE" >/dev/null
    print_done "Image ready"
  fi

  print_step "Creating container $CONTAINER_NAME"
  local docker_create_args=(
    create
    --name "$CONTAINER_NAME"
    --platform "$DOCKER_PLATFORM"
    -e PATH="$CONTAINER_PATH"
    -e RUSTUP_TOOLCHAIN=stable
    -v "$ROOT_DIR:/workspace/beetle"
    -w "$WORKDIR_IN_CONTAINER"
  )
  if uses_cross_toolchain; then
    docker_create_args+=(
      -e CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$TARGET_LINKER"
      -e CC_aarch64_unknown_linux_gnu="$TARGET_LINKER"
      -e AR_aarch64_unknown_linux_gnu="$TARGET_AR"
      -e PKG_CONFIG_ALLOW_CROSS=1
      -e PKG_CONFIG_LIBDIR="$TARGET_PKG_CONFIG_LIBDIR"
      -e PKG_CONFIG_SYSROOT_DIR=/
    )
  fi
  docker "${docker_create_args[@]}" "$IMAGE" sleep infinity >/dev/null
  print_done "Container created"
}

start_container() {
  print_step "Starting container $CONTAINER_NAME"
  docker start "$CONTAINER_NAME" >/dev/null
  print_done "Container started"
}

run_in_container() {
  local command="$1"
  docker exec "$CONTAINER_NAME" /bin/bash -lc "export PATH='$CONTAINER_PATH'; $command"
}

retry_in_container() {
  local command="$1"
  local attempts="${2:-5}"
  local delay_secs=2
  local attempt=1
  while true; do
    if run_in_container "$command" >/dev/null; then
      return 0
    fi
    if [ "$attempt" -ge "$attempts" ]; then
      return 1
    fi
    sleep "$delay_secs"
    attempt=$((attempt + 1))
  done
}

bootstrap_packages() {
  print_step "Installing Linux build dependencies"
  if uses_cross_toolchain; then
    local apt_packages="${CROSS_APT_PACKAGES[*]}"
    run_in_container \
      'dpkg --print-foreign-architectures | grep -qx arm64 || dpkg --add-architecture arm64' >/dev/null
    retry_in_container "apt-get update -o Acquire::Retries=5"
    retry_in_container "DEBIAN_FRONTEND=noninteractive apt-get install -y -o Acquire::Retries=5 --fix-missing $apt_packages libasound2-dev:arm64 libudev-dev:arm64"
  else
    local apt_packages="${NATIVE_APT_PACKAGES[*]}"
    retry_in_container "apt-get update -o Acquire::Retries=5"
    retry_in_container "DEBIAN_FRONTEND=noninteractive apt-get install -y -o Acquire::Retries=5 --fix-missing $apt_packages"
  fi
  print_done "System packages installed"
}

bootstrap_rust_target() {
  print_step "Installing Rust target $BUILD_TARGET"
  retry_in_container "rustup target add $BUILD_TARGET"
  print_done "Rust target installed"
}

verify_environment() {
  print_step "Verifying aarch64 GNU build environment"
  run_in_container "uname -a | sed -n '1p'"
  run_in_container "cargo -V"
  run_in_container "rustup target list --installed | grep -qx '$BUILD_TARGET'"
  if uses_cross_toolchain; then
    run_in_container "$TARGET_LINKER -dumpmachine"
  else
    run_in_container "rustc -vV | sed -n '1,6p'"
  fi
  run_in_container "pkg-config --modversion alsa"
  run_in_container "pkg-config --modversion libudev"
  print_done "GNU target to use: $BUILD_TARGET"
}

show_next_steps() {
  cat <<EOF

Linux aarch64 GNU build container is ready.

Container:
  $CONTAINER_NAME

Repository mount:
  $ROOT_DIR -> $WORKDIR_IN_CONTAINER

Enter the container:
  docker exec -it $CONTAINER_NAME /bin/bash

Build Beetle for Linux aarch64 GNU:
  docker exec -it $CONTAINER_NAME /bin/bash -lc 'cd $WORKDIR_IN_CONTAINER && TARGET=linux-aarch64 ./build.sh --no-deploy'

Build and package a Linux release bundle through the public entrypoint:
  docker exec -it $CONTAINER_NAME /bin/bash -lc 'cd $WORKDIR_IN_CONTAINER && TARGET=linux-aarch64 ./build.sh --package-linux'

Inside the container, Beetle lives at:
  $WORKDIR_IN_CONTAINER
EOF
}

main() {
  require_command docker
  require_docker_daemon
  if container_needs_recreate; then
    remove_existing_container
    create_container
  fi
  start_container
  bootstrap_packages
  bootstrap_rust_target
  verify_environment
  show_next_steps
}

main "$@"

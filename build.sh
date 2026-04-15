#!/usr/bin/env bash
# One-shot build script with interactive platform selection.
set -e
SCRIPT_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_ROOT"
# shellcheck source=/dev/null
source "$SCRIPT_ROOT/scripts/build_flash_strategy.sh"

# Colors (build + Linux SSH deploy)
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

show_help() {
  cat <<'EOF'
Usage:
  ./build.sh [--flash | --flash-update] [--no-monitor] [--no-deploy] [--deploy-linux]
             [--package-profile <name>] [cargo build args...]
  ./build.sh build-c6
  ./build.sh flash-c6
  ./build.sh flash-all [P4 build args...]

Linux SSH deploy only (no compile; needs an existing target/*/release/beetle):
  ./build.sh --deploy-linux

Hosted coprocessor firmware:
  ./build.sh build-c6
  ./build.sh flash-c6
  ./build.sh flash-all

Default (interactive TTY): after a successful build, asks whether to deploy:
  - Linux targets → SSH upload (same flow as --deploy-linux)
  - ESP targets   → USB flash (port + erase menu unless --flash-update)

Non-interactive / CI: use --no-deploy, BEETLE_SKIP_DEPLOY_PROMPT=1, or redirect stdin.

Skip the question and flash ESP immediately (automation):
  --flash          Build then flash ESP; interactive erase menu (default: update only, keep NVS).
  --flash-update   Build then flash ESP without erase (no erase menu).

Quick examples:
  ./build.sh
  ./build.sh --package-profile core-only
  TARGET=linux ./build.sh
  TARGET=linux ./build.sh --package-profile linux-full
  TARGET=linux-armv7 ./build.sh
  TARGET=linux-aarch64 ./build.sh
  ./scripts/create_linux_aarch64_build_docker.sh
  TARGET=esp ./build.sh
  TARGET=esp ./build.sh --package-profile voice
  TARGET=esp ./build.sh --flash
  ./build.sh --deploy-linux

Notes:
  - On macOS building Linux musl, auto mode uses Docker only if the daemon is running; otherwise musl-cross (Homebrew).
  - For option 5 beginner setup, run: ./scripts/create_linux_aarch64_build_docker.sh
  - Force local: BUILD_METHOD=local ./build.sh
  - Force Docker: BUILD_METHOD=docker ./build.sh
  - Force remote: BUILD_METHOD=remote ./build.sh
  - For Linux builds, this script uses Rust stable toolchain.
EOF
}

case "${1:-}" in
  build-c6|flash-c6|flash-all)
    exec "$SCRIPT_ROOT/scripts/esp_hosted_c6.sh" "$@"
    ;;
esac

# Fast-path help.
for arg in "$@"; do
  case "$arg" in
    -h|--help) show_help; exit 0 ;;
  esac
done

MSG_TITLE="Beetle Build Script"
MSG_SELECT_PLATFORM="Select build platform:"
MSG_PLATFORM_ESP_S3="ESP32-S3 Firmware (default)"
MSG_PLATFORM_ESP_P4="ESP32-P4 Firmware"
MSG_PLATFORM_LINUX="Linux x86_64"
MSG_PLATFORM_LINUX_ARMV7="Linux armv7 (32-bit ARM hard-float)"
MSG_PLATFORM_LINUX_AARCH64="Linux aarch64 (64-bit ARM)"
MSG_INPUT_OPTION="Enter option"
MSG_PRESS_ENTER="press Enter for"
MSG_INVALID_OPTION="Invalid option, enter"
MSG_LINUX_MODE="Linux Build Mode"
MSG_DETECTED_LINUX="Detected Linux system, using native build"
MSG_DETECTED_MACOS="Detected macOS system, cross-compiling to Linux"
MSG_SELECT_METHOD="Select build method:"
MSG_METHOD_DOCKER="Docker build (recommended, no setup needed)"
MSG_METHOD_MUSL="musl-cross toolchain (requires installation)"
MSG_ERROR_NO_DOCKER="Error: Docker not found"
MSG_INSTALL_DOCKER="Please install Docker Desktop"
MSG_UNKNOWN_OS="Warning: Unknown system"
MSG_TRY_NATIVE="trying native build"
MSG_ESP_MODE="ESP32 Build Mode"
MSG_USING_DOCKER="Using Docker for build"
MSG_BUILD_IN_DOCKER="Building in Docker"
MSG_BUILD_COMPLETE="Build complete"
MSG_BINARY="Binary"
# 固定 target 到本仓库，避免环境/IDE 将 CARGO_TARGET_DIR 指到临时目录导致 esp-idf-sys bindings 与 esp-idf-svc cfg 不一致。
export CARGO_TARGET_DIR="${SCRIPT_ROOT}/target"
export PATH="/usr/local/cargo/bin:${HOME}/.cargo/bin:${PATH}"

# --- Parse args (same as build.ps1) ---
DO_FLASH=""
DO_DEPLOY_LINUX=""
NO_MONITOR=""
NO_DEPLOY_PROMPT=""
FLASH_NO_ERASE=""
BUILD_METHOD="${BUILD_METHOD:-auto}" # auto | docker | local | remote
BUILD_PROFILE="release"
PACKAGE_PROFILE="${PACKAGE_PROFILE:-}"
BUILD_ARGS=()
REMOTE_BUILD_ROLE=""
REMOTE_BUILD_DIR=""
REMOTE_BUILD_BIN=""
REMOTE_BUILD_TARGET_ENV=""
REMOTE_BUILD_ACTIVE=0
REMOTE_TARGET_PREPARED=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)             show_help; exit 0 ;;
    --flash)               DO_FLASH=1 ;;
    --flash-update)        DO_FLASH=1; FLASH_NO_ERASE=1 ;;
    --no-monitor)          NO_MONITOR=1 ;;
    --no-deploy)           NO_DEPLOY_PROMPT=1 ;;
    --deploy-linux)        DO_DEPLOY_LINUX=1 ;;
    --package-profile)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --package-profile requires a value." >&2; exit 1; }
      PACKAGE_PROFILE="$1"
      ;;
    --package-profile=*)
      PACKAGE_PROFILE="${1#*=}"
      ;;
    *)
      BUILD_ARGS+=("$1")
      ;;
  esac
  shift
done

package_profile_features() {
  local profile="$1"
  case "$profile" in
    core-only)
      printf '%s\n' '--no-default-features --features default_runtime'
      ;;
    voice)
      printf '%s\n' '--no-default-features --features default_runtime,capability_voice'
      ;;
    vision)
      printf '%s\n' '--no-default-features --features default_runtime,capability_vision'
      ;;
    sensor)
      printf '%s\n' '--no-default-features --features default_runtime,capability_sensor'
      ;;
    voice+vision)
      printf '%s\n' '--no-default-features --features default_runtime,capability_voice,capability_vision'
      ;;
    voice+sensor)
      printf '%s\n' '--no-default-features --features default_runtime,capability_voice,capability_sensor'
      ;;
    vision+sensor)
      printf '%s\n' '--no-default-features --features default_runtime,capability_vision,capability_sensor'
      ;;
    voice+vision+sensor)
      printf '%s\n' '--no-default-features --features default_runtime,capability_voice,capability_vision,capability_sensor'
      ;;
    linux-full)
      printf '%s\n' '--no-default-features --features default_runtime,capability_voice,capability_vision,capability_sensor,capability_office'
      ;;
    *)
      echo "Error: unsupported package profile: $profile" >&2
      echo "Supported: core-only, voice, vision, sensor, voice+vision, voice+sensor, vision+sensor, voice+vision+sensor, linux-full" >&2
      exit 1
      ;;
  esac
}

default_package_profile_for_target() {
  local target="$1"
  if [[ "$target" =~ -unknown-linux ]]; then
    printf '%s\n' 'linux-full'
  else
    printf '%s\n' 'voice+vision+sensor'
  fi
}

target_mcu_from_triple() {
  local target="$1"
  case "$target" in
    xtensa-esp32-espidf) printf '%s\n' 'esp32' ;;
    xtensa-esp32s2-espidf) printf '%s\n' 'esp32s2' ;;
    xtensa-esp32s3-espidf) printf '%s\n' 'esp32s3' ;;
    riscv32imc-esp-espidf) printf '%s\n' 'esp32c3' ;;
    riscv32imac-esp-espidf) printf '%s\n' 'esp32c6' ;;
    riscv32imafc-esp-espidf) printf '%s\n' 'esp32p4' ;;
    *) return 1 ;;
  esac
}

default_sdkconfig_overlay_for_target() {
  local target="$1"
  case "$target" in
    xtensa-esp32s3-espidf) printf '%s\n' 'sdkconfig.defaults.esp32s3.board' ;;
    riscv32imafc-esp-espidf) printf '%s\n' 'sdkconfig.defaults.esp32p4.board' ;;
    *) return 1 ;;
  esac
}

list_flash_ports() {
  local ports=()
  local f
  if [[ "$(uname -s)" = "Linux" ]]; then
    for f in /dev/ttyUSB* /dev/ttyACM*; do [[ -e "$f" ]] && ports+=("$f"); done
  else
    for f in /dev/cu.usbmodem* /dev/cu.usbserial* /dev/cu.SLAB* /dev/cu.wchusbserial* /dev/cu.UART*; do [[ -e "$f" ]] && ports+=("$f"); done
  fi
  printf '%s\n' "${ports[@]}"
}

linux_detect_pkg_manager() {
    local pm
    for pm in apt-get dnf yum pacman zypper apk; do
        if command -v "$pm" >/dev/null 2>&1; then
            printf '%s\n' "$pm"
            return 0
        fi
    done
    return 1
}

run_with_privilege() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
        return $?
    fi
    if command -v sudo >/dev/null 2>&1; then
        sudo "$@"
        return $?
    fi
    return 1
}

print_linux_build_prereq_help() {
    local pm="$1"
    echo ""
    echo "Linux native build prerequisites are missing."
    echo "This build needs: compiler toolchain, pkg-config, ALSA headers, libudev headers, and Rust."
    case "$pm" in
        apt-get)
            echo "Run:"
            echo "  sudo apt-get update"
            echo "  sudo apt-get install -y ca-certificates curl wget build-essential pkg-config libasound2-dev libudev-dev"
            ;;
        dnf)
            echo "Run:"
            echo "  sudo dnf install -y ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-lib-devel systemd-devel"
            ;;
        yum)
            echo "Run:"
            echo "  sudo yum install -y ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-lib-devel systemd-devel"
            ;;
        pacman)
            echo "Run:"
            echo "  sudo pacman -Sy --noconfirm ca-certificates curl wget base-devel pkgconf alsa-lib systemd"
            ;;
        zypper)
            echo "Run:"
            echo "  sudo zypper --non-interactive install ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-devel systemd-devel"
            ;;
        apk)
            echo "Run:"
            echo "  sudo apk add --no-cache ca-certificates curl wget build-base pkgconf alsa-lib-dev eudev-dev"
            ;;
        *)
            echo "Install a compiler, pkg-config, ALSA development headers, libudev/systemd development headers, curl or wget, and Rust."
            ;;
    esac
    echo "Rust toolchain:"
    echo "  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable"
    echo ""
}

install_linux_build_prereqs() {
    local pm="$1"
    case "$pm" in
        apt-get)
            run_with_privilege env DEBIAN_FRONTEND=noninteractive apt-get update || return 1
            run_with_privilege env DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates curl wget build-essential pkg-config libasound2-dev libudev-dev || return 1
            ;;
        dnf)
            run_with_privilege dnf install -y ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-lib-devel systemd-devel || return 1
            ;;
        yum)
            run_with_privilege yum install -y ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-lib-devel systemd-devel || return 1
            ;;
        pacman)
            run_with_privilege pacman -Sy --noconfirm ca-certificates curl wget base-devel pkgconf alsa-lib systemd || return 1
            ;;
        zypper)
            run_with_privilege zypper --non-interactive install ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-devel systemd-devel || return 1
            ;;
        apk)
            run_with_privilege apk add --no-cache ca-certificates curl wget build-base pkgconf alsa-lib-dev eudev-dev || return 1
            ;;
        *)
            return 1
            ;;
    esac
}

ensure_local_rust_toolchain() {
    export PATH="${HOME}/.cargo/bin:${PATH}"
    if command -v cargo >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
        return 0
    fi

    echo ""
    echo "========== Preparing Rust Toolchain =========="

    if ! command -v rustup >/dev/null 2>&1; then
        if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
            if [[ "$(uname -s)" == "Linux" ]]; then
                local pm=""
                pm=$(linux_detect_pkg_manager 2>/dev/null || true)
                if [[ -n "$pm" ]]; then
                    echo "  curl/wget not found. Installing Linux build prerequisites first ..."
                    install_linux_build_prereqs "$pm" || {
                        print_linux_build_prereq_help "$pm"
                        echo "Error: failed to install Linux build prerequisites automatically." >&2
                        exit 1
                    }
                fi
            fi
        fi

        if command -v curl >/dev/null 2>&1; then
            echo "  Installing rustup + stable toolchain ..."
            curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable
        elif command -v wget >/dev/null 2>&1; then
            echo "  Installing rustup + stable toolchain ..."
            wget -qO- https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
        else
            echo "Error: neither curl nor wget is available, so Rust cannot be installed automatically." >&2
            if [[ "$(uname -s)" == "Linux" ]]; then
                print_linux_build_prereq_help "$(linux_detect_pkg_manager 2>/dev/null || true)"
            else
                echo "Install Rust from: https://rustup.rs" >&2
            fi
            exit 1
        fi
    fi

    export PATH="${HOME}/.cargo/bin:${PATH}"
    if ! command -v cargo >/dev/null 2>&1 || ! command -v rustup >/dev/null 2>&1; then
        echo "Error: Rust toolchain is still unavailable after rustup install." >&2
        echo "Install Rust from: https://rustup.rs" >&2
        exit 1
    fi
}

ensure_local_stable_toolchain() {
    ensure_local_rust_toolchain

    if cargo +stable -V >/dev/null 2>&1 && rustup +stable target list >/dev/null 2>&1; then
        return 0
    fi

    echo ""
    echo "========== Preparing Stable Rust Toolchain =========="
    rustup toolchain install stable --profile minimal

    if ! cargo +stable -V >/dev/null 2>&1 || ! rustup +stable target list >/dev/null 2>&1; then
        echo "Error: stable Rust toolchain is still unavailable after rustup install." >&2
        exit 1
    fi
}

linux_musl_linker_for_target() {
    local candidate
    case "$1" in
        x86_64-unknown-linux-musl)
            for candidate in x86_64-linux-musl-gcc x86_64-unknown-linux-musl-gcc; do
                if command -v "$candidate" >/dev/null 2>&1; then
                    printf '%s\n' "$candidate"
                    return 0
                fi
            done
            printf '%s\n' "x86_64-linux-musl-gcc"
            ;;
        armv7-unknown-linux-musleabihf)
            for candidate in arm-linux-musleabihf-gcc armv7-unknown-linux-musleabihf-gcc; do
                if command -v "$candidate" >/dev/null 2>&1; then
                    printf '%s\n' "$candidate"
                    return 0
                fi
            done
            printf '%s\n' "arm-linux-musleabihf-gcc"
            ;;
        aarch64-unknown-linux-musl)
            for candidate in aarch64-linux-musl-gcc aarch64-unknown-linux-musl-gcc; do
                if command -v "$candidate" >/dev/null 2>&1; then
                    printf '%s\n' "$candidate"
                    return 0
                fi
            done
            printf '%s\n' "aarch64-linux-musl-gcc"
            ;;
        *) return 1 ;;
    esac
}

ensure_linux_musl_linker_config() {
    local target="$1"
    local linker=""

    linker=$(linux_musl_linker_for_target "$target") || return 0

    mkdir -p .cargo
    if ! grep -Fq "$target" .cargo/config.toml 2>/dev/null; then
        cat >> .cargo/config.toml <<EOF

[target.$target]
linker = "$linker"
EOF
        echo "  Configured musl linker in .cargo/config.toml for $target"
    fi

    case "$target" in
        x86_64-unknown-linux-musl)
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="$linker"
            ;;
        armv7-unknown-linux-musleabihf)
            export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER="$linker"
            ;;
        aarch64-unknown-linux-musl)
            export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$linker"
            ;;
    esac
}

print_linux_musl_build_prereq_help() {
    local target="$1"
    local linker="$2"

    echo ""
    echo "Linux musl build prerequisites are missing for target: $target"
    echo "This build needs the matching musl cross linker:"
    echo "  $linker"
    echo ""
    echo "Recommended options:"
    echo "  1) Use the matching musl build container/toolchain for this target."
    echo "  2) Install the matching musl cross compiler and re-run ./build.sh."
    echo ""
}

ensure_local_linux_native_build_prereqs() {
    local pm=""

    if [[ "$(uname -s)" != "Linux" ]]; then
        return 0
    fi
    if [[ ! "$BUILD_TARGET" =~ -unknown-linux-gnu$ ]]; then
        return 0
    fi

    if command -v cc >/dev/null 2>&1 \
        && command -v pkg-config >/dev/null 2>&1 \
        && pkg-config --exists alsa libudev 2>/dev/null; then
        return 0
    fi

    pm=$(linux_detect_pkg_manager 2>/dev/null || true)
    echo ""
    echo "========== Preparing Native Linux Build Environment =========="

    if [[ -n "$pm" ]]; then
        echo "  Installing missing native build prerequisites using $pm ..."
        install_linux_build_prereqs "$pm" || {
            print_linux_build_prereq_help "$pm"
            echo "Error: failed to install Linux build prerequisites automatically." >&2
            exit 1
        }
    else
        print_linux_build_prereq_help ""
        echo "Error: unsupported package manager for automatic prerequisite install." >&2
        exit 1
    fi
}

ensure_local_linux_musl_build_prereqs() {
    local linker=""

    if [[ "$(uname -s)" != "Linux" ]]; then
        return 0
    fi
    if [[ ! "$BUILD_TARGET" =~ -unknown-linux-musl ]]; then
        return 0
    fi

    linker=$(linux_musl_linker_for_target "$BUILD_TARGET") || return 0
    if ! command -v "$linker" >/dev/null 2>&1; then
        print_linux_musl_build_prereq_help "$BUILD_TARGET" "$linker"
        echo "Error: required musl linker is unavailable for $BUILD_TARGET." >&2
        exit 1
    fi

    ensure_linux_musl_linker_config "$BUILD_TARGET"
}

# --- Linux SSH deploy (merged from former deploy-linux.sh) ---
linux_deploy_fetch_embed_deps_from_url() {
    if [ -z "${BEETLE_EMBED_DEPS_URL:-}" ]; then
        return 0
    fi
    case "$BEETLE_EMBED_DEPS_URL" in
        https://* | http://*) ;;
        *)
            echo -e "${RED}BEETLE_EMBED_DEPS_URL must be http(s)${NC}"
            exit 1
            ;;
    esac

    local dest="$SCRIPT_ROOT/packaging/linux/embed-deps/$EMBED_DEPS_ARCH"
    mkdir -p "$dest"

    echo "========== Fetch embed-deps (BEETLE_EMBED_DEPS_URL) =========="
    echo ""
    echo "  URL: $BEETLE_EMBED_DEPS_URL"
    echo "  → $dest"
    echo ""

    local tmp
    tmp=$(mktemp "${TMPDIR:-/tmp}/beetle-deps.XXXXXX")
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$BEETLE_EMBED_DEPS_URL" -o "$tmp"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$tmp" "$BEETLE_EMBED_DEPS_URL"
    else
        echo -e "${RED}Install curl or wget to use BEETLE_EMBED_DEPS_URL${NC}"
        rm -f "$tmp"
        exit 1
    fi

    if [ -n "${BEETLE_EMBED_DEPS_SHA256:-}" ]; then
        local got
        got=$(shasum -a 256 "$tmp" | awk '{print $1}')
        if [ "$got" != "$BEETLE_EMBED_DEPS_SHA256" ]; then
            echo -e "${RED}SHA256 mismatch (expected $BEETLE_EMBED_DEPS_SHA256 got $got)${NC}"
            rm -f "$tmp"
            exit 1
        fi
        echo -e "${GREEN}✓ SHA256 OK${NC}"
    fi

    local exdir
    exdir=$(mktemp -d "${TMPDIR:-/tmp}/beetle-deps-ex.XXXXXX")
    if ! tar -xf "$tmp" -C "$exdir" 2>/dev/null; then
        echo -e "${RED}Failed to extract archive (need .tar / .tar.gz / .tar.xz)${NC}"
        rm -rf "$exdir" "$tmp"
        exit 1
    fi
    rm -f "$tmp"

    local n=0
    local f
    while IFS= read -r f; do
        case "$(basename "$f")" in
            iw | hostapd | dnsmasq)
                cp -f "$f" "$dest/"
                chmod +x "$dest/$(basename "$f")"
                n=$((n + 1))
                ;;
        esac
    done < <(find "$exdir" -type f \( -name iw -o -name hostapd -o -name dnsmasq \) 2>/dev/null)

    rm -rf "$exdir"

    if [ "$n" -lt 1 ]; then
        echo -e "${YELLOW}Warning: archive contained no iw/hostapd/dnsmasq; nothing copied.${NC}"
    else
        echo -e "${GREEN}✓ Placed $n helper(s) under packaging/linux/embed-deps/$EMBED_DEPS_ARCH/${NC}"
    fi
    echo ""
}

# Detect available binaries
linux_deploy_detect_binaries() {
    local binaries=()

    if [ -f "target/x86_64-unknown-linux-musl/release/beetle" ]; then
        binaries+=("x86_64")
    fi

    if [ -f "target/x86_64-unknown-linux-gnu/release/beetle" ]; then
        binaries+=("x86_64-gnu")
    fi

    if [ -f "target/armv7-unknown-linux-musleabihf/release/beetle" ]; then
        binaries+=("armv7")
    fi

    if [ -f "target/armv7-unknown-linux-gnueabihf/release/beetle" ]; then
        binaries+=("armv7-gnu")
    fi

    if [ -f "target/aarch64-unknown-linux-musl/release/beetle" ]; then
        binaries+=("aarch64")
    fi

    if [ -f "target/aarch64-unknown-linux-gnu/release/beetle" ]; then
        binaries+=("aarch64-gnu")
    fi

    echo "${binaries[@]}"
}

linux_deploy_array_contains() {
    local needle="$1"
    shift
    local item
    for item in "$@"; do
        if [ "$item" = "$needle" ]; then
            return 0
        fi
    done
    return 1
}

linux_set_paths_for_selected_arch() {
    case "$SELECTED_ARCH" in
        x86_64)
            BINARY_PATH="target/x86_64-unknown-linux-musl/release/beetle"
            EMBED_DEPS_ARCH="x86_64"
            ;;
        x86_64-gnu)
            BINARY_PATH="target/x86_64-unknown-linux-gnu/release/beetle"
            EMBED_DEPS_ARCH="x86_64"
            ;;
        armv7)
            BINARY_PATH="target/armv7-unknown-linux-musleabihf/release/beetle"
            EMBED_DEPS_ARCH="armv7"
            ;;
        armv7-gnu)
            BINARY_PATH="target/armv7-unknown-linux-gnueabihf/release/beetle"
            EMBED_DEPS_ARCH="armv7"
            ;;
        aarch64)
            BINARY_PATH="target/aarch64-unknown-linux-musl/release/beetle"
            EMBED_DEPS_ARCH="aarch64"
            ;;
        aarch64-gnu)
            BINARY_PATH="target/aarch64-unknown-linux-gnu/release/beetle"
            EMBED_DEPS_ARCH="aarch64"
            ;;
        *)
            echo -e "${RED}Error: unknown arch key: $SELECTED_ARCH${NC}"
            exit 1
            ;;
    esac
}

linux_selected_arch_from_target() {
    case "$1" in
        x86_64-unknown-linux-musl) echo "x86_64" ;;
        x86_64-unknown-linux-gnu) echo "x86_64-gnu" ;;
        armv7-unknown-linux-musleabihf) echo "armv7" ;;
        armv7-unknown-linux-gnueabihf) echo "armv7-gnu" ;;
        aarch64-unknown-linux-musl) echo "aarch64" ;;
        aarch64-unknown-linux-gnu) echo "aarch64-gnu" ;;
        *) return 1 ;;
    esac
}

# Select architecture
linux_deploy_select_arch() {
    local available=($(linux_deploy_detect_binaries))
    local recommended_arch=""

    if [ ${#available[@]} -eq 0 ]; then
        echo -e "${RED}Error: No compiled binaries found${NC}"
        echo "Please run ./build.sh first"
        exit 1
    fi

    case "${DEVICE_ARCH:-}" in
        x86_64)
            if linux_deploy_array_contains "x86_64" "${available[@]}"; then
                recommended_arch="x86_64"
            elif linux_deploy_array_contains "x86_64-gnu" "${available[@]}"; then
                recommended_arch="x86_64-gnu"
            fi
            ;;
        armv7l)
            if linux_deploy_array_contains "armv7" "${available[@]}"; then
                recommended_arch="armv7"
            elif linux_deploy_array_contains "armv7-gnu" "${available[@]}"; then
                recommended_arch="armv7-gnu"
            fi
            ;;
        aarch64)
            if linux_deploy_array_contains "aarch64" "${available[@]}"; then
                recommended_arch="aarch64"
            elif linux_deploy_array_contains "aarch64-gnu" "${available[@]}"; then
                recommended_arch="aarch64-gnu"
            fi
            ;;
    esac

    echo "Available builds:"
    local i=1
    for arch in "${available[@]}"; do
        echo "  $i) $arch"
        ((i++))
    done
    echo ""

    if [ ${#available[@]} -eq 1 ]; then
        SELECTED_ARCH="${available[0]}"
        echo -e "${GREEN}Auto-selected: $SELECTED_ARCH${NC}"
    elif [ -n "$recommended_arch" ]; then
        SELECTED_ARCH="$recommended_arch"
        echo -e "${GREEN}Auto-selected for device architecture ${DEVICE_ARCH}: $SELECTED_ARCH${NC}"
    else
        read -p "Select architecture [1-${#available[@]}]: " choice
        choice=${choice:-1}
        SELECTED_ARCH="${available[$((choice-1))]}"
    fi

    linux_set_paths_for_selected_arch

    echo ""
}

# Persist last successful target (IP / user / port). Password is never stored.
# Path: ~/.config/beetle/deploy-linux.defaults (mode 600).
DEPLOY_DEFAULTS_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/beetle"
DEPLOY_DEFAULTS_FILE="$DEPLOY_DEFAULTS_DIR/deploy-linux.defaults"
DEPLOY_ROOT="/opt/beetle"
DEPLOY_RELEASES_DIR="$DEPLOY_ROOT/releases"
DEPLOY_CURRENT_LINK="$DEPLOY_ROOT/current"
DEPLOY_ROLLBACK_LINK="$DEPLOY_ROOT/rollback"
DEPLOY_GLOBAL_BIN="/usr/local/bin/beetle"
DEPLOY_STATE_DIR="/var/lib/beetle"
DEPLOY_SERVICE_PATH="/etc/systemd/system/beetle.service"
DEPLOY_INIT_PATH="/etc/init.d/beetle"
DEPLOY_ENV_PATH="/etc/default/beetle"
PRIVILEGED_PREFIX=""

linux_remote_default_build_dir() {
    local remote_user="${1:-root}"
    if [ "$remote_user" = "root" ]; then
        printf '%s\n' "/root/beetle-build"
    else
        printf '/home/%s/beetle-build\n' "$remote_user"
    fi
}

linux_deploy_load_deploy_defaults() {
    local saw_remote_build_dir=0
    DEFAULT_DEVICE_IP=""
    DEFAULT_DEVICE_USER="root"
    DEFAULT_SSH_PORT="22"
    DEFAULT_REMOTE_BUILD_DIR="$(linux_remote_default_build_dir "$DEFAULT_DEVICE_USER")"
    if [ ! -f "$DEPLOY_DEFAULTS_FILE" ]; then
        return 0
    fi
    while IFS= read -r line || [ -n "$line" ]; do
        case "$line" in
            ''|\#*) continue ;;
        esac
        case "$line" in
            DEVICE_IP=*) DEFAULT_DEVICE_IP="${line#DEVICE_IP=}" ;;
            DEVICE_USER=*) DEFAULT_DEVICE_USER="${line#DEVICE_USER=}" ;;
            SSH_PORT=*) DEFAULT_SSH_PORT="${line#SSH_PORT=}" ;;
            REMOTE_BUILD_DIR=*) DEFAULT_REMOTE_BUILD_DIR="${line#REMOTE_BUILD_DIR=}"; saw_remote_build_dir=1 ;;
        esac
    done <"$DEPLOY_DEFAULTS_FILE"
    if [ "$saw_remote_build_dir" != "1" ]; then
        DEFAULT_REMOTE_BUILD_DIR="$(linux_remote_default_build_dir "$DEFAULT_DEVICE_USER")"
    fi
}

linux_deploy_save_deploy_defaults() {
    mkdir -p "$DEPLOY_DEFAULTS_DIR"
    (
        umask 077
        {
            echo "# beetle build.sh linux deploy — last successful target (do not commit)"
            printf 'DEVICE_IP=%s\n' "$DEVICE_IP"
            printf 'DEVICE_USER=%s\n' "$DEVICE_USER"
            printf 'SSH_PORT=%s\n' "$SSH_PORT"
            printf 'REMOTE_BUILD_DIR=%s\n' "${REMOTE_BUILD_DIR:-$DEFAULT_REMOTE_BUILD_DIR}"
        } >"${DEPLOY_DEFAULTS_FILE}.tmp"
        mv "${DEPLOY_DEFAULTS_FILE}.tmp" "$DEPLOY_DEFAULTS_FILE"
    )
}

# Input remote host information
linux_deploy_input_device_info() {
    echo "========== Target Host Information =========="
    echo ""
    linux_deploy_load_deploy_defaults
    local default_remote_build_dir_saved="${DEFAULT_REMOTE_BUILD_DIR}"

    if [ -n "$DEFAULT_DEVICE_IP" ]; then
        echo -e "${GREEN}Saved host: ${DEFAULT_DEVICE_USER}@${DEFAULT_DEVICE_IP}:${DEFAULT_SSH_PORT}${NC}"
        echo "(Press Enter to keep; password is not saved — use SSH keys for passwordless login)"
        echo ""
    fi

    if [ -n "$DEFAULT_DEVICE_IP" ]; then
        read -p "Host IP address [$DEFAULT_DEVICE_IP]: " DEVICE_IP
    else
        read -p "Host IP address: " DEVICE_IP
    fi
    DEVICE_IP=${DEVICE_IP:-$DEFAULT_DEVICE_IP}
    if [ -z "$DEVICE_IP" ]; then
        echo -e "${RED}Error: IP address cannot be empty${NC}"
        exit 1
    fi

    read -p "Username [${DEFAULT_DEVICE_USER}]: " DEVICE_USER
    DEVICE_USER=${DEVICE_USER:-$DEFAULT_DEVICE_USER}
    if [ "$default_remote_build_dir_saved" = "$(linux_remote_default_build_dir "$DEFAULT_DEVICE_USER")" ]; then
        DEFAULT_REMOTE_BUILD_DIR="$(linux_remote_default_build_dir "$DEVICE_USER")"
    fi

    read -p "SSH port [${DEFAULT_SSH_PORT}]: " SSH_PORT
    SSH_PORT=${SSH_PORT:-$DEFAULT_SSH_PORT}

    if ! [[ "$SSH_PORT" =~ ^[0-9]+$ ]] || [ "$SSH_PORT" -lt 1 ] || [ "$SSH_PORT" -gt 65535 ]; then
        echo -e "${RED}Error: SSH port must be a number between 1 and 65535${NC}"
        exit 1
    fi

    echo ""
    echo -e "${BLUE}Target host: ${DEVICE_USER}@${DEVICE_IP}:${SSH_PORT}${NC}"
    echo ""
}

linux_prepare_remote_target() {
    if [ "${REMOTE_TARGET_PREPARED:-0}" = "1" ]; then
        return 0
    fi
    linux_deploy_input_device_info
    linux_deploy_setup_ssh_mux
    trap linux_deploy_cleanup_ssh_mux EXIT INT TERM
    linux_deploy_test_connection
    linux_deploy_detect_device_arch
    REMOTE_TARGET_PREPARED=1
}

linux_remote_prepare_privileged_prefix() {
    if ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        'test "$(id -u)" -eq 0' >/dev/null 2>&1; then
        PRIVILEGED_PREFIX=""
        return 0
    fi

    if ! ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        'command -v sudo >/dev/null 2>&1' >/dev/null 2>&1; then
        echo -e "${RED}Error: remote deploy needs root or sudo for /opt, /var/lib, and /etc/systemd/system${NC}" >&2
        return 1
    fi

    if ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        'sudo -n true' >/dev/null 2>&1; then
        PRIVILEGED_PREFIX="sudo"
        return 0
    fi

    echo "========== Remote Privilege Check =========="
    echo "Remote deploy needs sudo to write /opt/beetle, /var/lib/beetle, and service files."
    echo "If prompted, enter the remote sudo password for ${DEVICE_USER}@${DEVICE_IP}."
    echo ""
    ssh -tt "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        'sudo -v' || {
            echo -e "${RED}Error: sudo authentication failed on the remote host${NC}" >&2
            return 1
        }

    PRIVILEGED_PREFIX="sudo"
}

linux_remote_run_script_with_optional_sudo() {
    local remote_cmd="$1"
    if [ -n "${PRIVILEGED_PREFIX:-}" ]; then
        ssh -tt "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
            "$PRIVILEGED_PREFIX $remote_cmd"
    else
        ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
            "$remote_cmd"
    fi
}

linux_remote_input_build_dir() {
    echo "========== Remote Build Directory =========="
    echo ""
    linux_deploy_load_deploy_defaults
    read -p "Remote project directory [${DEFAULT_REMOTE_BUILD_DIR}]: " REMOTE_BUILD_DIR
    REMOTE_BUILD_DIR=${REMOTE_BUILD_DIR:-$DEFAULT_REMOTE_BUILD_DIR}
    case "$REMOTE_BUILD_DIR" in
        ""|"/")
            echo -e "${RED}Error: remote project directory must not be empty or /${NC}"
            exit 1
            ;;
        /dev/*)
            echo -e "${RED}Error: remote project directory must be a mounted directory, not a /dev block device path${NC}"
            echo -e "${YELLOW}Use something like /root/beetle-build or /opt/beetle-build${NC}"
            exit 1
            ;;
        *"'"*)
            echo -e "${RED}Error: remote project directory must not contain single quotes${NC}"
            exit 1
            ;;
    esac
    echo ""
    echo -e "${BLUE}Remote project directory: ${REMOTE_BUILD_DIR}${NC}"
    echo ""
    linux_deploy_save_deploy_defaults
}

linux_remote_select_build_role() {
    local default_role="1"
    echo "========== Remote Build Result =========="
    echo ""
    echo "  1) Build + deploy on this host"
    echo "  2) Build on remote, pull artifact back to local, then continue SSH deploy"
    echo "  3) Build on remote only; leave artifact on remote"
    echo ""
    read -p "Select mode [1-3] (default ${default_role}): " remote_role
    remote_role=${remote_role:-$default_role}
    case "$remote_role" in
        1) REMOTE_BUILD_ROLE="deploy" ;;
        2) REMOTE_BUILD_ROLE="pull_deploy" ;;
        3) REMOTE_BUILD_ROLE="leave" ;;
        *)
            echo -e "${RED}Error: invalid remote build mode: $remote_role${NC}"
            exit 1
            ;;
    esac
    echo ""
}

linux_apply_docker_target_for_platform() {
    case "$PLATFORM_CHOICE" in
        3) BUILD_TARGET="x86_64-unknown-linux-musl" ;;
        4) BUILD_TARGET="armv7-unknown-linux-musleabihf" ;;
        5) BUILD_TARGET="aarch64-unknown-linux-musl" ;;
    esac
}

linux_prepare_remote_build_context() {
    linux_prepare_remote_target
    linux_remote_input_build_dir
    linux_remote_select_build_role

    local selected_family=""
    local remote_family=""

    case "$PLATFORM_CHOICE" in
        3) selected_family="x86_64" ;;
        4) selected_family="armv7" ;;
        5) selected_family="aarch64" ;;
        *)
            echo -e "${RED}Error: remote build only supports Linux targets${NC}"
            exit 1
            ;;
    esac

    case "${DEVICE_ARCH:-}" in
        x86_64)
            remote_family="x86_64"
            BUILD_TARGET="x86_64-unknown-linux-gnu"
            REMOTE_BUILD_TARGET_ENV="linux"
            ;;
        armv7l|armv6l)
            remote_family="armv7"
            BUILD_TARGET="armv7-unknown-linux-gnueabihf"
            REMOTE_BUILD_TARGET_ENV="linux-armv7"
            ;;
        aarch64|arm64)
            remote_family="aarch64"
            BUILD_TARGET="aarch64-unknown-linux-gnu"
            REMOTE_BUILD_TARGET_ENV="linux-aarch64"
            ;;
        *)
            echo -e "${RED}Error: unsupported remote Linux architecture: ${DEVICE_ARCH:-unknown}${NC}"
            exit 1
            ;;
    esac

    if [ "$selected_family" != "$remote_family" ]; then
        echo -e "${RED}Error: selected platform does not match remote host architecture${NC}"
        echo "  Selected platform family: $selected_family"
        echo "  Remote host architecture: ${DEVICE_ARCH:-unknown}"
        echo "Remote build currently supports native builds on a matching remote Linux host."
        exit 1
    fi

    SELECTED_ARCH=$(linux_selected_arch_from_target "$BUILD_TARGET") || {
        echo -e "${RED}Error: unsupported remote build target: $BUILD_TARGET${NC}"
        exit 1
    }
    linux_set_paths_for_selected_arch
}

# Reuse one SSH connection for the whole script so password (or keyboard-interactive)
# is not prompted on every ssh/scp invocation. Requires OpenSSH client.
# macOS: $TMPDIR is often under /var/folders/...; ControlPath = dir + %C + ssh suffix
# can exceed AF_UNIX sun_path (~104 bytes). Prefer /tmp (short path); %C keeps names compact.
linux_deploy_setup_ssh_mux() {
    if [ -d /tmp ] && [ -w /tmp ]; then
        SSH_MUX_DIR=$(mktemp -d /tmp/bd.XXXXXX)
    else
        SSH_MUX_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bd.XXXXXX")
    fi
    chmod 700 "$SSH_MUX_DIR"
    SSH_MUX_OPTS=(
        -o "ControlMaster=auto"
        -o "ControlPath=$SSH_MUX_DIR/%C"
        -o "ControlPersist=300"
    )
}

linux_deploy_cleanup_ssh_mux() {
    if [ -n "${SSH_MUX_DIR:-}" ] && [ -n "${DEVICE_USER:-}" ] && [ -n "${DEVICE_IP:-}" ]; then
        ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" -o BatchMode=yes -O exit \
            "${DEVICE_USER}@${DEVICE_IP}" 2>/dev/null || true
    fi
    if [ -n "${SSH_MUX_DIR:-}" ] && [ -d "$SSH_MUX_DIR" ]; then
        rm -rf "$SSH_MUX_DIR"
    fi
}

# Test connection
linux_deploy_test_connection() {
    echo "========== Testing Connection =========="
    echo ""

    if ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" -o ConnectTimeout=5 -o BatchMode=yes \
        "${DEVICE_USER}@${DEVICE_IP}" "echo 'OK'" &>/dev/null; then
        echo -e "${GREEN}✓ SSH connection successful${NC}"
    else
        echo -e "${YELLOW}⚠ SSH connection failed, password may be required${NC}"
        echo "Testing connection..."
        if ! ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" "echo 'OK'"; then
            echo -e "${RED}Error: Cannot connect to device${NC}"
            exit 1
        fi
    fi

    linux_deploy_save_deploy_defaults

    echo ""
}

# Detect device architecture
linux_deploy_detect_device_arch() {
    echo "========== Detecting Device =========="
    echo ""

    # One remote shell; keep bash-3.2 portability for macOS /bin/bash.
    _uname_out=$(
        ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
            "uname -m; uname -s" 2>/dev/null || printf '%s\n' unknown unknown
    )
    DEVICE_ARCH=$(printf '%s\n' "$_uname_out" | sed -n '1p')
    DEVICE_OS=$(printf '%s\n' "$_uname_out" | sed -n '2p')
    DEVICE_ARCH=${DEVICE_ARCH:-unknown}
    DEVICE_OS=${DEVICE_OS:-unknown}

    echo "Device architecture: $DEVICE_ARCH"
    echo "Operating system: $DEVICE_OS"

    echo ""
}

linux_deploy_probe_remote_install_state() {
    echo "========== Remote Install State =========="
    echo ""

    REMOTE_HAS_SYSTEMD=0
    REMOTE_HAS_SERVICE=0
    REMOTE_SERVICE_ACTIVE=0
    REMOTE_SERVICE_ENABLED=0
    REMOTE_HAS_CURRENT_BIN=0
    REMOTE_HAS_GLOBAL_BIN=0
    REMOTE_CURRENT_TARGET=""

    local out line key value
    out=$(
        ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" '
            if command -v systemctl >/dev/null 2>&1; then
                echo HAS_SYSTEMD=1
            else
                echo HAS_SYSTEMD=0
            fi
            if [ -e /etc/systemd/system/beetle.service ]; then
                echo HAS_SERVICE=1
            else
                echo HAS_SERVICE=0
            fi
            if [ -x /opt/beetle/current/beetle ]; then
                echo HAS_CURRENT_BIN=1
            else
                echo HAS_CURRENT_BIN=0
            fi
            if [ -x /usr/local/bin/beetle ]; then
                echo HAS_GLOBAL_BIN=1
            else
                echo HAS_GLOBAL_BIN=0
            fi
            if [ -L /opt/beetle/current ]; then
                target=$(readlink -f /opt/beetle/current 2>/dev/null || readlink /opt/beetle/current 2>/dev/null || true)
                echo CURRENT_TARGET=$target
            else
                echo CURRENT_TARGET=
            fi
            if command -v systemctl >/dev/null 2>&1 && [ -e /etc/systemd/system/beetle.service ]; then
                if systemctl is-active --quiet beetle; then
                    echo SERVICE_ACTIVE=1
                else
                    echo SERVICE_ACTIVE=0
                fi
                if systemctl is-enabled --quiet beetle 2>/dev/null; then
                    echo SERVICE_ENABLED=1
                else
                    echo SERVICE_ENABLED=0
                fi
            else
                echo SERVICE_ACTIVE=0
                echo SERVICE_ENABLED=0
            fi
        ' 2>/dev/null
    )

    while IFS= read -r line; do
        key=${line%%=*}
        value=${line#*=}
        case "$key" in
            HAS_SYSTEMD) REMOTE_HAS_SYSTEMD=${value:-0} ;;
            HAS_SERVICE) REMOTE_HAS_SERVICE=${value:-0} ;;
            SERVICE_ACTIVE) REMOTE_SERVICE_ACTIVE=${value:-0} ;;
            SERVICE_ENABLED) REMOTE_SERVICE_ENABLED=${value:-0} ;;
            HAS_CURRENT_BIN) REMOTE_HAS_CURRENT_BIN=${value:-0} ;;
            HAS_GLOBAL_BIN) REMOTE_HAS_GLOBAL_BIN=${value:-0} ;;
            CURRENT_TARGET) REMOTE_CURRENT_TARGET=$value ;;
        esac
    done <<<"$out"

    if [ "$REMOTE_HAS_SYSTEMD" = "1" ]; then
        echo "  Service manager: systemd"
    else
        echo "  Service manager: none detected"
    fi
    if [ "$REMOTE_HAS_SERVICE" = "1" ]; then
        echo "  beetle.service: present"
    else
        echo "  beetle.service: missing"
    fi
    if [ "$REMOTE_HAS_CURRENT_BIN" = "1" ]; then
        echo "  Current release: $DEPLOY_CURRENT_LINK"
    else
        echo "  Current release: missing"
    fi
    if [ -n "$REMOTE_CURRENT_TARGET" ]; then
        echo "  Current target: $REMOTE_CURRENT_TARGET"
    fi
    if [ "$REMOTE_HAS_GLOBAL_BIN" = "1" ]; then
        echo "  Global command: $DEPLOY_GLOBAL_BIN present"
    fi
    if [ "$REMOTE_HAS_SERVICE" = "1" ]; then
        if [ "$REMOTE_SERVICE_ACTIVE" = "1" ]; then
            echo "  Service status: active"
        elif [ "$REMOTE_SERVICE_ENABLED" = "1" ]; then
            echo "  Service status: installed but not running"
        else
            echo "  Service status: installed and disabled/stopped"
        fi
    fi
    echo ""
}

# Select deployment mode
linux_deploy_select_deploy_mode() {
    local default_mode="2"
    if [ "${REMOTE_HAS_SERVICE:-0}" = "1" ] || [ "${REMOTE_HAS_CURRENT_BIN:-0}" = "1" ] || [ "${REMOTE_HAS_GLOBAL_BIN:-0}" = "1" ]; then
        default_mode="3"
    fi

    echo "========== Deployment Mode =========="
    echo ""
    echo "  1) Quick deploy (binary only; do not touch services)"
    echo "  2) Full deploy (install/refresh service, enable + start)"
    echo "  3) Smart update (replace binary; restart existing service when appropriate)"
    echo ""
    if [ "$default_mode" = "3" ]; then
        echo "Detected an existing beetle install on the device; defaulting to smart update."
    else
        echo "No existing beetle install detected; defaulting to full deploy."
    fi
    if [ "${REMOTE_HAS_SYSTEMD:-0}" != "1" ]; then
        echo "systemd was not detected; full deploy will install the SysV init example when possible."
    fi
    echo ""

    read -p "Select mode [1-3] (default ${default_mode}): " mode
    DEPLOY_MODE=${mode:-$default_mode}
    echo ""
}

# True if packaging/linux/embed-deps/<arch>/ has at least one non-doc file.
linux_deploy_local_embed_deps_nonempty() {
    local embed="$SCRIPT_ROOT/packaging/linux/embed-deps/$EMBED_DEPS_ARCH"
    local f
    if [ ! -d "$embed" ]; then
        return 1
    fi
    for f in "$embed"/*; do
        [ -f "$f" ] || continue
        case "$(basename "$f")" in
            README*|*.md|*.txt) continue ;;
        esac
        return 0
    done
    return 1
}

# Remote: any of iw/hostapd/dnsmasq missing on PATH → sets REMOTE_WIFI_INCOMPLETE=1 else 0
linux_deploy_probe_remote_wifi_tools() {
    REMOTE_WIFI_INCOMPLETE=0
    local out
    out=$(
        ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
            'for c in iw hostapd dnsmasq; do
                command -v "$c" >/dev/null 2>&1 || echo MISSING
            done'
    )
    case "$out" in
        *MISSING*) REMOTE_WIFI_INCOMPLETE=1 ;;
    esac
}

# Before upload: if device lacks tools and this PC has no embed-deps, ask to continue or abort.
linux_deploy_prompt_wifi_helpers_or_continue() {
    echo "========== WiFi helper tools (preflight) =========="
    echo ""
    linux_deploy_probe_remote_wifi_tools
    if [ "${REMOTE_WIFI_INCOMPLETE:-0}" -eq 0 ]; then
        echo -e "${GREEN}  Device already has iw, hostapd, and dnsmasq on PATH — nothing extra to bundle.${NC}"
        echo ""
        return 0
    fi
    if linux_deploy_local_embed_deps_nonempty; then
        echo -e "${GREEN}  This PC has files under packaging/linux/embed-deps/$EMBED_DEPS_ARCH/ — they will be uploaded to /opt/beetle/bin/.${NC}"
        echo ""
        return 0
    fi
    echo -e "${YELLOW}  The device is missing one or more of: iw, hostapd, dnsmasq (on PATH).${NC}"
    echo -e "${YELLOW}  Beetle’s Linux WiFi needs them; this script does not download or pull from firmware.${NC}"
    echo ""
    echo "  To bundle helpers: put binaries named iw, hostapd, dnsmasq on **this computer** under:"
    echo "    $SCRIPT_ROOT/packaging/linux/embed-deps/$EMBED_DEPS_ARCH/"
    echo ""
    read -p "  Deploy beetle binary only (no WiFi helpers this time)? [Y/n]: " wifi_ans
    wifi_ans=${wifi_ans:-Y}
    case "$wifi_ans" in
        [Nn]*)
            echo ""
            echo "Aborted. Add the three tools to embed-deps (or install them on the device), then run ./build.sh --deploy-linux again."
            exit 0
            ;;
    esac
    echo ""
}

# Optional bundled WiFi userland (iw, hostapd, dnsmasq) for distros without opkg/apk/apt.
# Place binaries in packaging/linux/embed-deps/<arch>/ (same arch as selected build).
EMBED_DEPS_UPLOADED=0
linux_deploy_upload_embed_deps() {
    local embed="$SCRIPT_ROOT/packaging/linux/embed-deps/$EMBED_DEPS_ARCH"
    local remote_stage_dir=""
    local remote_cmd=""
    EMBED_DEPS_UPLOADED=0
    if [ ! -d "$embed" ]; then
        return 0
    fi
    local has=""
    local f
    for f in "$embed"/*; do
        [ -f "$f" ] || continue
        case "$(basename "$f")" in
            README*|*.md|*.txt) continue ;;
        esac
        has=1
        break
    done
    if [ -z "$has" ]; then
        return 0
    fi
    echo "Uploading bundled WiFi tools → /opt/beetle/bin ..."
    remote_stage_dir="/tmp/beetle-embed-deps-$EMBED_DEPS_ARCH"
    ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        "rm -rf '$remote_stage_dir' && mkdir -p '$remote_stage_dir'" || return 1
    for f in "$embed"/*; do
        [ -f "$f" ] || continue
        case "$(basename "$f")" in
            README*|*.md|*.txt) continue ;;
        esac
        scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" "$f" \
            "${DEVICE_USER}@${DEVICE_IP}:${remote_stage_dir}/" || return 1
    done
    remote_cmd="env REMOTE_STAGE_DIR='$remote_stage_dir' sh -s"
    linux_remote_run_script_with_optional_sudo "$remote_cmd" << 'REMOTE_EOF' || return 1
set -eu

mkdir -p /opt/beetle/bin
for f in "$REMOTE_STAGE_DIR"/*; do
    [ -f "$f" ] || continue
    mv "$f" /opt/beetle/bin/
done
for f in /opt/beetle/bin/*; do
    [ -f "$f" ] && chmod a+x "$f"
done
rm -rf "$REMOTE_STAGE_DIR"
REMOTE_EOF
    EMBED_DEPS_UPLOADED=1
    echo -e "${GREEN}✓ Bundled tools uploaded (beetle prefers /opt/beetle/bin)${NC}"
}

# Upload files
linux_deploy_upload_files() {
    echo "========== Uploading Files =========="
    echo ""

    DEPLOY_RELEASE_NAME="${BEETLE_DEPLOY_RELEASE_NAME:-$(date +%Y%m%d-%H%M%S)-$SELECTED_ARCH}"
    REMOTE_TMP_BIN="/tmp/beetle-${DEPLOY_RELEASE_NAME}.bin"
    REMOTE_TMP_SERVICE="/tmp/beetle-${DEPLOY_RELEASE_NAME}.service"
    REMOTE_TMP_INIT="/tmp/beetle-${DEPLOY_RELEASE_NAME}.init"
    REMOTE_TMP_ENV="/tmp/beetle-${DEPLOY_RELEASE_NAME}.env"
    REMOTE_TMP_README="/tmp/beetle-${DEPLOY_RELEASE_NAME}.README.txt"
    REMOTE_TMP_HWJSON="/tmp/beetle-${DEPLOY_RELEASE_NAME}.hardware.json"

    linux_deploy_upload_embed_deps || return 1

    echo "Uploading release payload for ${DEPLOY_RELEASE_NAME} ..."
    if [ "${REMOTE_BUILD_ACTIVE:-0}" = "1" ] && [ "${REMOTE_BUILD_ROLE:-}" = "deploy" ] && [ -n "${REMOTE_BUILD_BIN:-}" ]; then
        echo "Using remote-built binary: $REMOTE_BUILD_BIN"
        ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
            "cp '$REMOTE_BUILD_BIN' '$REMOTE_TMP_BIN' && chmod 755 '$REMOTE_TMP_BIN'"
    else
        scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" "$BINARY_PATH" \
            "${DEVICE_USER}@${DEVICE_IP}:${REMOTE_TMP_BIN}"
    fi
    scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" packaging/linux/README.txt \
        "${DEVICE_USER}@${DEVICE_IP}:${REMOTE_TMP_README}"
    scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" packaging/linux/hardware.json.example \
        "${DEVICE_USER}@${DEVICE_IP}:${REMOTE_TMP_HWJSON}"

    if [ "$DEPLOY_MODE" = "2" ]; then
        echo "Uploading service templates..."
        scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" packaging/linux/beetle.service \
            "${DEVICE_USER}@${DEVICE_IP}:${REMOTE_TMP_SERVICE}"
        scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" packaging/linux/beetle.init \
            "${DEVICE_USER}@${DEVICE_IP}:${REMOTE_TMP_INIT}"
        scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" packaging/linux/beetle.env.example \
            "${DEVICE_USER}@${DEVICE_IP}:${REMOTE_TMP_ENV}"
    fi

    echo -e "${GREEN}✓ Upload complete${NC}"
    echo ""
}

# Shell-side check: script never "extracts from firmware"; it only uploads files you placed
# under packaging/linux/embed-deps/<arch>/ on this computer.
linux_deploy_report_wifi_tools_on_device() {
    echo "========== WiFi tools on device (iw / hostapd / dnsmasq) =========="
    echo ""
    local miss=0
    local line
    while IFS= read -r line; do
        echo "  $line"
        case "$line" in
            *MISS*) miss=1 ;;
        esac
    done < <(
        ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
            'for c in iw hostapd dnsmasq; do
                if command -v "$c" >/dev/null 2>&1; then
                    p=$(command -v "$c")
                    echo "OK $c → $p"
                else
                    echo "MISS $c"
                fi
            done'
    )
    echo ""
    if [ "${EMBED_DEPS_UPLOADED:-0}" -eq 1 ]; then
        echo -e "${GREEN}  Also uploaded from this PC: packaging/linux/embed-deps/$EMBED_DEPS_ARCH/ → /opt/beetle/bin/${NC}"
        echo ""
        return 0
    fi
    if [ "$miss" -eq 1 ]; then
        echo -e "${YELLOW}  This deploy script does NOT auto-copy tools from device firmware.${NC}"
        echo -e "${YELLOW}  Put matching binaries on **this computer** under:${NC}"
        echo -e "${YELLOW}    packaging/linux/embed-deps/$EMBED_DEPS_ARCH/${NC}"
        echo -e "${YELLOW}  then run ./build.sh --deploy-linux again (or install those packages on the device if you can).${NC}"
        echo -e "${YELLOW}  See docs/zh-cn/linux-release-rollback.md${NC}"
        echo ""
    fi
}

# Install release layout and optional service templates
linux_deploy_install_payloads() {
    echo "========== Installing Payload =========="
    echo ""

    local remote_cmd="env DEPLOY_ROOT='$DEPLOY_ROOT' DEPLOY_RELEASES_DIR='$DEPLOY_RELEASES_DIR' DEPLOY_CURRENT_LINK='$DEPLOY_CURRENT_LINK' DEPLOY_ROLLBACK_LINK='$DEPLOY_ROLLBACK_LINK' DEPLOY_GLOBAL_BIN='$DEPLOY_GLOBAL_BIN' DEPLOY_STATE_DIR='$DEPLOY_STATE_DIR' DEPLOY_SERVICE_PATH='$DEPLOY_SERVICE_PATH' DEPLOY_INIT_PATH='$DEPLOY_INIT_PATH' DEPLOY_ENV_PATH='$DEPLOY_ENV_PATH' DEPLOY_RELEASE_NAME='$DEPLOY_RELEASE_NAME' REMOTE_TMP_BIN='$REMOTE_TMP_BIN' REMOTE_TMP_SERVICE='$REMOTE_TMP_SERVICE' REMOTE_TMP_INIT='$REMOTE_TMP_INIT' REMOTE_TMP_ENV='$REMOTE_TMP_ENV' REMOTE_TMP_README='$REMOTE_TMP_README' REMOTE_TMP_HWJSON='$REMOTE_TMP_HWJSON' sh -s"
    linux_remote_run_script_with_optional_sudo "$remote_cmd" << 'REMOTE_EOF'
set -eu

release_dir="$DEPLOY_RELEASES_DIR/$DEPLOY_RELEASE_NAME"
mkdir -p "$DEPLOY_RELEASES_DIR" "$DEPLOY_ROOT/bin" "$DEPLOY_STATE_DIR" "$DEPLOY_STATE_DIR/config" "$(dirname "$DEPLOY_GLOBAL_BIN")"
chmod 700 "$DEPLOY_STATE_DIR" "$DEPLOY_STATE_DIR/config" 2>/dev/null || true

previous_release=""
if [ -e "$DEPLOY_CURRENT_LINK" ]; then
    previous_release="$(readlink -f "$DEPLOY_CURRENT_LINK" 2>/dev/null || true)"
fi

if [ -e "$release_dir" ]; then
    rm -rf "$release_dir"
fi
mkdir -p "$release_dir"

[ -f "$REMOTE_TMP_BIN" ] || {
    echo "Uploaded binary missing: $REMOTE_TMP_BIN" >&2
    exit 1
}

if [ -x "$DEPLOY_CURRENT_LINK/beetle" ]; then
    cp -pf "$DEPLOY_CURRENT_LINK/beetle" "$DEPLOY_ROOT/beetle.prev" || true
fi

chmod 755 "$REMOTE_TMP_BIN"
mv "$REMOTE_TMP_BIN" "$release_dir/beetle"

if [ -f "$REMOTE_TMP_README" ]; then
    mv "$REMOTE_TMP_README" "$release_dir/README.txt"
fi
if [ -f "$REMOTE_TMP_HWJSON" ]; then
    mv "$REMOTE_TMP_HWJSON" "$release_dir/hardware.json.example"
    if [ ! -f "$DEPLOY_STATE_DIR/config/hardware.json.example" ]; then
        cp -f "$release_dir/hardware.json.example" "$DEPLOY_STATE_DIR/config/hardware.json.example"
    fi
fi

if [ -e "$DEPLOY_CURRENT_LINK" ] && [ ! -L "$DEPLOY_CURRENT_LINK" ]; then
    rm -rf "$DEPLOY_CURRENT_LINK"
fi
ln -sfn "$release_dir" "$DEPLOY_CURRENT_LINK"
rollback_release=""
if [ -n "$previous_release" ] && [ "$previous_release" != "$release_dir" ] && [ -d "$previous_release" ]; then
    ln -sfn "$previous_release" "$DEPLOY_ROLLBACK_LINK"
    rollback_release="$(readlink -f "$DEPLOY_ROLLBACK_LINK" 2>/dev/null || true)"
else
    rm -f "$DEPLOY_ROLLBACK_LINK"
fi
ln -sfn "$DEPLOY_CURRENT_LINK/beetle" "$DEPLOY_GLOBAL_BIN"
if [ -f "$release_dir/README.txt" ]; then
    ln -sfn "$DEPLOY_CURRENT_LINK/README.txt" "$DEPLOY_ROOT/README.txt"
fi
rm -f "$DEPLOY_ROOT/beetle"

release_state_dir="$DEPLOY_STATE_DIR/runtime/linux_release"
release_state_path="$release_state_dir/state.json"
state_schema_path="$DEPLOY_STATE_DIR/runtime/state_schema.json"
mkdir -p "$release_state_dir" "$(dirname "$state_schema_path")"
updated_at="$(date -u +%s)"
current_name="$(basename "$release_dir")"
rollback_name=""
if [ -n "$rollback_release" ]; then
    rollback_name="$(basename "$rollback_release")"
fi
{
    printf '{\n'
    printf '  "version": 1,\n'
    printf '  "deploy_root": "%s",\n' "$DEPLOY_ROOT"
    printf '  "current": {\n'
    printf '    "name": "%s",\n' "$current_name"
    printf '    "path": "%s"\n' "$release_dir"
    printf '  },\n'
    if [ -n "$rollback_release" ]; then
        printf '  "rollback": {\n'
        printf '    "name": "%s",\n' "$rollback_name"
        printf '    "path": "%s"\n' "$rollback_release"
        printf '  },\n'
    else
        printf '  "rollback": null,\n'
    fi
    printf '  "rollout_state": "pending_validation",\n'
    printf '  "last_updated_at": %s,\n' "$updated_at"
    printf '  "last_action": "deploy_release"\n'
    printf '}\n'
} > "$release_state_path"
{
    printf '{\n'
    printf '  "version": 1,\n'
    printf '  "updated_at": %s\n' "$updated_at"
    printf '}\n'
} > "$state_schema_path"

if [ -f "$REMOTE_TMP_SERVICE" ]; then
    mv "$REMOTE_TMP_SERVICE" "$DEPLOY_SERVICE_PATH"
    chmod 644 "$DEPLOY_SERVICE_PATH"
fi
if [ -f "$REMOTE_TMP_INIT" ] && [ -d /etc/init.d ]; then
    mv "$REMOTE_TMP_INIT" "$DEPLOY_INIT_PATH"
    chmod 755 "$DEPLOY_INIT_PATH"
fi
if [ -f "$REMOTE_TMP_ENV" ]; then
    if [ ! -f "$DEPLOY_ENV_PATH" ]; then
        mv "$REMOTE_TMP_ENV" "$DEPLOY_ENV_PATH"
        chmod 644 "$DEPLOY_ENV_PATH"
    else
        rm -f "$REMOTE_TMP_ENV"
    fi
fi

rm -f "$REMOTE_TMP_SERVICE" "$REMOTE_TMP_INIT" "$REMOTE_TMP_ENV" "$REMOTE_TMP_README" "$REMOTE_TMP_HWJSON"

echo "✓ Installed release: $release_dir"
echo "✓ Current symlink: $DEPLOY_CURRENT_LINK -> $release_dir"
if [ -n "$rollback_release" ]; then
    echo "✓ Rollback symlink: $DEPLOY_ROLLBACK_LINK -> $rollback_release"
else
    echo "✓ Rollback symlink: none"
fi
echo "✓ Global command: $DEPLOY_GLOBAL_BIN -> $DEPLOY_CURRENT_LINK/beetle"
REMOTE_EOF

    echo ""
}

linux_deploy_manage_service() {
    echo "========== Service Handling =========="
    echo ""

    if [ "$DEPLOY_MODE" = "1" ]; then
        echo "Quick deploy selected; skipping service changes."
        echo ""
        return 0
    fi

    local remote_cmd="env DEPLOY_MODE='$DEPLOY_MODE' DEPLOY_SERVICE_PATH='$DEPLOY_SERVICE_PATH' DEPLOY_INIT_PATH='$DEPLOY_INIT_PATH' sh -s"
    linux_remote_run_script_with_optional_sudo "$remote_cmd" << 'REMOTE_EOF'
set -eu

if command -v systemctl >/dev/null 2>&1; then
    if [ "$DEPLOY_MODE" = "2" ]; then
        if [ ! -f "$DEPLOY_SERVICE_PATH" ]; then
            echo "Expected service file missing: $DEPLOY_SERVICE_PATH" >&2
            exit 1
        fi
        systemctl daemon-reload
        systemctl enable beetle >/dev/null 2>&1 || true
        if systemctl is-active --quiet beetle; then
            systemctl restart beetle
            echo "✓ beetle service restarted"
        else
            systemctl start beetle
            echo "✓ beetle service started"
        fi
    elif [ -f "$DEPLOY_SERVICE_PATH" ]; then
        systemctl daemon-reload
        if systemctl is-active --quiet beetle; then
            systemctl restart beetle
            echo "✓ beetle service restarted"
        elif systemctl is-enabled --quiet beetle 2>/dev/null; then
            systemctl start beetle
            echo "✓ beetle service started"
        else
            echo "beetle.service exists but is disabled/stopped; left unchanged."
        fi
    else
        echo "No beetle.service on device; binary updated only."
    fi
elif [ "$DEPLOY_MODE" = "2" ] && [ -f "$DEPLOY_INIT_PATH" ]; then
    echo "systemd not detected; installed init example at $DEPLOY_INIT_PATH"
else
    echo "No service manager automation available; binary updated only."
fi
REMOTE_EOF

    echo ""
}

linux_deploy_verify_remote_install() {
    echo "========== Remote Verification =========="
    echo ""

    local remote_cmd="env DEPLOY_MODE='$DEPLOY_MODE' DEPLOY_ROOT='$DEPLOY_ROOT' DEPLOY_CURRENT_LINK='$DEPLOY_CURRENT_LINK' DEPLOY_GLOBAL_BIN='$DEPLOY_GLOBAL_BIN' DEPLOY_STATE_DIR='$DEPLOY_STATE_DIR' DEPLOY_SERVICE_PATH='$DEPLOY_SERVICE_PATH' sh -s"
    ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        "$remote_cmd" << 'REMOTE_EOF'
set -eu

missing=0

echo "Paths:"
if [ -L "$DEPLOY_CURRENT_LINK" ]; then
    target=$(readlink -f "$DEPLOY_CURRENT_LINK" 2>/dev/null || readlink "$DEPLOY_CURRENT_LINK" 2>/dev/null || true)
    echo "  current -> ${target:-$DEPLOY_CURRENT_LINK}"
else
    echo "  current -> (missing)"
    missing=1
fi
if [ -e "$DEPLOY_GLOBAL_BIN" ]; then
    ls -l "$DEPLOY_GLOBAL_BIN"
else
    echo "  global command -> missing"
    missing=1
fi
if [ -e "$DEPLOY_SERVICE_PATH" ]; then
    echo "  service file -> $DEPLOY_SERVICE_PATH"
elif [ "$DEPLOY_MODE" = "2" ]; then
    echo "  service file -> missing"
    missing=1
fi
if [ -d "$DEPLOY_STATE_DIR" ]; then
    echo "  state dir -> $DEPLOY_STATE_DIR"
else
    echo "  state dir -> missing"
    missing=1
fi
echo ""

if command -v systemctl >/dev/null 2>&1 && [ -f "$DEPLOY_SERVICE_PATH" ]; then
    exec_start_line=$(grep -E '^ExecStart=' "$DEPLOY_SERVICE_PATH" 2>/dev/null || true)
    if [ -n "$exec_start_line" ]; then
        echo "  ExecStart -> ${exec_start_line#ExecStart=}"
        case "$exec_start_line" in
            *"/beetle run"|*" beetle run")
                ;;
            *)
                echo "  WARNING: service unit does not pass the required 'run' subcommand."
                echo "  WARNING: refresh the unit file with a full deploy, or edit ExecStart to append ' run'."
                ;;
        esac
        echo ""
    fi
    if systemctl is-active --quiet beetle; then
        echo "Service: active"
    elif systemctl is-enabled --quiet beetle 2>/dev/null; then
        echo "Service: installed but not running"
    else
        echo "Service: installed and disabled/stopped"
    fi
    echo ""
    systemctl --no-pager --full status beetle 2>&1 | sed -n "1,12p" || true
    if ! systemctl is-active --quiet beetle; then
        echo ""
        echo "Recent journal:"
        journalctl -u beetle -n 20 --no-pager 2>&1 || true
    fi
fi

[ "$missing" -eq 0 ] || exit 1
REMOTE_EOF

    echo ""
}

# Show next steps
linux_deploy_show_next_steps() {
    echo "=========================================="
    echo "  Deployment Complete!"
    echo "=========================================="
    echo ""
    echo "Next steps:"
    echo ""
    if [ "${EMBED_DEPS_UPLOADED:-0}" -eq 1 ]; then
        echo "  (Bundled iw/hostapd/dnsmasq are under /opt/beetle/bin on the device.)"
        echo "  Beetle looks there first — you do not need to edit PATH on the device for these."
        echo ""
    fi
    echo "  1. Main paths:"
    echo "     - Current release: $DEPLOY_CURRENT_LINK"
    echo "     - Global command: $DEPLOY_GLOBAL_BIN"
    echo "     - State directory: $DEPLOY_STATE_DIR"
    echo "     - Service config: $DEPLOY_SERVICE_PATH"
    echo ""
    echo "  2. Start beetle manually if needed:"
    echo "     - Direct run: beetle run"
    echo "     - Or full path: $DEPLOY_CURRENT_LINK/beetle run"
    echo "     - Or: nohup beetle run >> /var/log/beetle.log 2>&1 &"
    echo ""
    if [ "${REMOTE_HAS_SYSTEMD:-0}" = "1" ] && [ "${REMOTE_HAS_SERVICE:-0}" = "1" ]; then
        if [ "${REMOTE_SERVICE_ACTIVE:-0}" = "1" ]; then
            echo "  3. systemd service is already active; restart if needed:"
            echo "     systemctl restart beetle"
        elif [ "${REMOTE_SERVICE_ENABLED:-0}" = "1" ]; then
            echo "  3. systemd service is installed but currently stopped:"
            echo "     systemctl start beetle"
        else
            echo "  3. systemd service is installed but disabled:"
            echo "     systemctl enable --now beetle"
        fi
        echo ""
        echo "  4. Configure WiFi (after WiFi stack works):"
    else
        echo "  3. Configure WiFi (after WiFi stack works):"
    fi
    echo "     Hotspot SSID Beetle → http://DEVICE_IP/ (or http://192.168.4.1 on SoftAP)"
    echo ""
}

# 主流程
linux_deploy_main() {
    echo ""
    echo "=========================================="
    echo "  Beetle Linux Deployment"
    echo "=========================================="
    echo ""
    linux_prepare_remote_target
    linux_remote_prepare_privileged_prefix || return 1
    if [ "${REMOTE_BUILD_ACTIVE:-0}" = "1" ] && [ "${REMOTE_BUILD_ROLE:-}" = "deploy" ]; then
        echo "Using remote-built ${SELECTED_ARCH} artifact from ${REMOTE_BUILD_BIN}"
        echo ""
    else
        linux_deploy_select_arch
    fi
    linux_deploy_fetch_embed_deps_from_url || return 1
    linux_deploy_probe_remote_install_state || return 1
    linux_deploy_select_deploy_mode
    linux_deploy_prompt_wifi_helpers_or_continue || return 1
    linux_deploy_upload_files || return 1
    linux_deploy_install_payloads || return 1
    linux_deploy_manage_service || return 1
    linux_deploy_report_wifi_tools_on_device || return 1
    linux_deploy_verify_remote_install || return 1
    linux_deploy_show_next_steps
}


if [[ -n "$DO_DEPLOY_LINUX" ]]; then
  linux_deploy_main
  exit $?
fi

run_linux_docker_build() {
  local target="$1"
  echo "  $MSG_USING_DOCKER"
  echo ""
  echo "========== $MSG_BUILD_IN_DOCKER =========="
  if [[ "$target" == "x86_64-unknown-linux-musl" ]]; then
    docker run --rm -e RUSTUP_TOOLCHAIN=stable -v "$SCRIPT_ROOT":/workspace -w /workspace \
      rust:latest \
      bash -c "rustup target add x86_64-unknown-linux-musl && cargo build --release --target x86_64-unknown-linux-musl"
  elif [[ "$target" == "armv7-unknown-linux-musleabihf" ]]; then
    docker run --rm -e RUSTUP_TOOLCHAIN=stable -v "$SCRIPT_ROOT":/home/rust/src -w /home/rust/src \
      messense/rust-musl-cross:armv7-musleabihf \
      cargo build --release --target armv7-unknown-linux-musleabihf
  elif [[ "$target" == "aarch64-unknown-linux-musl" ]]; then
    docker run --rm -e RUSTUP_TOOLCHAIN=stable -v "$SCRIPT_ROOT":/home/rust/src -w /home/rust/src \
      messense/rust-musl-cross:aarch64-musl \
      cargo build --release --target aarch64-unknown-linux-musl
  else
    echo "Error: Docker build not supported for target: $target" >&2
    exit 1
  fi
}

linux_remote_sync_entries() {
  # Remote Linux build only needs the actual Cargo project inputs.
  # Never archive the workspace root: this repo also contains local SDKs,
  # caches, docs, and experiments that can be tens of GB.
  local path
  for path in \
    .cargo \
    Cargo.toml \
    Cargo.lock \
    build.rs \
    build.sh \
    cfg.toml \
    rust-toolchain.toml \
    board_presets.toml \
    components \
    components_esp32s3.lock \
    packaging \
    partitions.csv \
    partitions_8mb.csv \
    partitions_32mb.csv \
    partitions_p4_16mb.csv \
    sdkconfig.defaults \
    sdkconfig.defaults.esp32s3 \
    sdkconfig.defaults.esp32s3.8mb.board \
    sdkconfig.defaults.esp32s3.board \
    sdkconfig.defaults.esp32s3.32mb.board \
    sdkconfig.defaults.esp32p4 \
    sdkconfig.defaults.esp32p4.board \
    src \
    third_party/esp-idf-hal \
    third_party/esp-idf-svc \
    third_party/esp-idf-sys \
    third_party/espressif__esp-dsp
  do
    if [ -e "$SCRIPT_ROOT/$path" ] || [ -L "$SCRIPT_ROOT/$path" ]; then
      printf '%s\n' "$path"
    fi
  done
}

linux_remote_sync_payload_kb() {
  local total=0
  local path
  local size

  while IFS= read -r path; do
    [ -n "$path" ] || continue
    size=$(du -sk "$SCRIPT_ROOT/$path" 2>/dev/null | awk 'NR==1 { print $1 + 0 }')
    total=$((total + size))
  done < <(linux_remote_sync_entries)

  printf '%s\n' "$total"
}

run_linux_remote_build() {
  local sync_manifest
  local payload_kb
  local payload_mib

  sync_manifest=$(mktemp "${TMPDIR:-/tmp}/beetle-remote-sync.XXXXXX")
  linux_remote_sync_entries > "$sync_manifest"
  if [ ! -s "$sync_manifest" ]; then
    rm -f "$sync_manifest"
    echo "Error: remote sync manifest is empty" >&2
    exit 1
  fi

  payload_kb=$(linux_remote_sync_payload_kb)
  payload_mib=$(((payload_kb + 1023) / 1024))

  echo "  Remote build over SSH"
  echo ""
  echo "========== Sync Project To Remote =========="
  echo "  Host: ${DEVICE_USER}@${DEVICE_IP}:${SSH_PORT}"
  echo "  Dir:  ${REMOTE_BUILD_DIR}"
  echo "  Set:  Cargo project inputs only"
  echo "  Size: ~${payload_mib} MiB"
  echo ""

  ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
    "REMOTE_BUILD_DIR='$REMOTE_BUILD_DIR' PAYLOAD_KB='$payload_kb' sh -s" << 'REMOTE_EOF'
set -eu

case "$REMOTE_BUILD_DIR" in
    ""|"/")
        echo "Refusing to sync to unsafe remote build directory: $REMOTE_BUILD_DIR" >&2
        exit 1
        ;;
    /dev/*)
        echo "Refusing to sync to block device path: $REMOTE_BUILD_DIR" >&2
        exit 1
        ;;
esac

parent_dir=$(dirname "$REMOTE_BUILD_DIR")
mkdir -p "$parent_dir"
rm -rf "$REMOTE_BUILD_DIR"
mkdir -p "$REMOTE_BUILD_DIR"
avail_kb=$(df -Pk "$parent_dir" | awk 'NR==2 { print $4 + 0 }')
need_kb=$((PAYLOAD_KB + 262144))
if [ "$avail_kb" -lt "$need_kb" ]; then
    echo "Not enough free space on remote filesystem after clearing old build dir: available ${avail_kb}KB, need at least ${need_kb}KB." >&2
    exit 1
fi
REMOTE_EOF

  COPYFILE_DISABLE=1 tar \
    --disable-copyfile \
    --no-xattrs \
    --no-mac-metadata \
    --exclude='.DS_Store' \
    --exclude='*/.DS_Store' \
    --exclude='._*' \
    --exclude='*/._*' \
    -cf - -T "$sync_manifest" | ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
      "tar -xf - -C '$REMOTE_BUILD_DIR'"
  rm -f "$sync_manifest"

  echo -e "${GREEN}✓ Source synced${NC}"
  echo ""
  echo "========== Build On Remote =========="
  echo "  Target: $BUILD_TARGET"
  echo ""

  ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
    "REMOTE_BUILD_DIR='$REMOTE_BUILD_DIR' REMOTE_TARGET_ENV='$REMOTE_BUILD_TARGET_ENV' sh -s" << 'REMOTE_EOF'
set -eu

ensure_remote_linux_build_prereqs() {
  need_pkg=0
  pkg_manager=""
  install_prefix=""

  if ! command -v cc >/dev/null 2>&1; then
    need_pkg=1
  fi
  if ! command -v pkg-config >/dev/null 2>&1; then
    need_pkg=1
  elif ! pkg-config --exists alsa libudev 2>/dev/null; then
    need_pkg=1
  fi
  if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
    need_pkg=1
  fi

  if [ "$need_pkg" -eq 0 ]; then
    return 0
  fi

  for pm in apt-get dnf yum pacman zypper apk; do
    if command -v "$pm" >/dev/null 2>&1; then
      pkg_manager="$pm"
      break
    fi
  done
  if [ -z "$pkg_manager" ]; then
    echo "Error: remote host is missing build prerequisites (compiler/pkg-config/alsa/libudev/curl), and no supported package manager was found." >&2
    exit 1
  fi

  if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
      install_prefix="sudo"
    else
      echo "Error: remote host needs build prerequisites; rerun with root or a user that has sudo." >&2
      exit 1
    fi
  fi

  echo "  Installing remote build prerequisites via $pkg_manager ..."
  case "$pkg_manager" in
    apt-get)
      ${install_prefix:+$install_prefix } env DEBIAN_FRONTEND=noninteractive apt-get update
      ${install_prefix:+$install_prefix } env DEBIAN_FRONTEND=noninteractive apt-get install -y ca-certificates curl wget build-essential pkg-config libasound2-dev libudev-dev
      ;;
    dnf)
      ${install_prefix:+$install_prefix } dnf install -y ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-lib-devel systemd-devel
      ;;
    yum)
      ${install_prefix:+$install_prefix } yum install -y ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-lib-devel systemd-devel
      ;;
    pacman)
      ${install_prefix:+$install_prefix } pacman -Sy --noconfirm ca-certificates curl wget base-devel pkgconf alsa-lib systemd
      ;;
    zypper)
      ${install_prefix:+$install_prefix } zypper --non-interactive install ca-certificates curl wget gcc gcc-c++ make pkgconf-pkg-config alsa-devel systemd-devel
      ;;
    apk)
      ${install_prefix:+$install_prefix } apk add --no-cache ca-certificates curl wget build-base pkgconf alsa-lib-dev eudev-dev
      ;;
  esac
}

ensure_remote_rust_toolchain() {
  export PATH="$HOME/.cargo/bin:$PATH"
  if command -v cargo >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
    return 0
  fi

  if ! command -v rustup >/dev/null 2>&1; then
    installer=""
    if command -v curl >/dev/null 2>&1; then
      installer='curl https://sh.rustup.rs -sSf'
    elif command -v wget >/dev/null 2>&1; then
      installer='wget -qO- https://sh.rustup.rs'
    else
      echo "Error: rustup is missing and neither curl nor wget is available to install it." >&2
      exit 1
    fi

    echo "  Installing rustup + stable toolchain on remote host ..."
    sh -c "$installer" | sh -s -- -y --profile minimal --default-toolchain stable
  fi

  export PATH="$HOME/.cargo/bin:$PATH"
  command -v cargo >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1 || {
    echo "Error: Rust toolchain is still unavailable after rustup install." >&2
    exit 1
  }
}

ensure_remote_stable_toolchain() {
  ensure_remote_rust_toolchain

  if cargo +stable -V >/dev/null 2>&1 && rustup +stable target list >/dev/null 2>&1; then
    return 0
  fi

  echo "  Installing or repairing remote stable Rust toolchain ..."
  rustup toolchain install stable --profile minimal

  if ! cargo +stable -V >/dev/null 2>&1 || ! rustup +stable target list >/dev/null 2>&1; then
    echo "Error: remote stable Rust toolchain is still unavailable after rustup install." >&2
    exit 1
  fi
}

cd "$REMOTE_BUILD_DIR"
ensure_remote_linux_build_prereqs
ensure_remote_stable_toolchain
TARGET="$REMOTE_TARGET_ENV" BUILD_METHOD=local BEETLE_SKIP_DEPLOY_PROMPT=1 ./build.sh --no-deploy
REMOTE_EOF

  ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
    "[ -f '$REMOTE_BUILD_BIN' ]" || {
      echo "Error: remote build finished but artifact not found: $REMOTE_BUILD_BIN" >&2
      exit 1
    }
}

linux_remote_pull_artifact_to_local() {
  echo ""
  echo "========== Fetch Remote Artifact =========="
  echo "  Remote: $REMOTE_BUILD_BIN"
  echo "  Local:  $BIN"
  echo ""
  mkdir -p "$(dirname "$BIN")"
  scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" \
    "${DEVICE_USER}@${DEVICE_IP}:${REMOTE_BUILD_BIN}" "$BIN"
  chmod 755 "$BIN" 2>/dev/null || true
  echo -e "${GREEN}✓ Artifact fetched to local workspace${NC}"
  echo ""
}

linux_running_in_container() {
  [[ -f /.dockerenv ]] && return 0
  [[ -f /run/.containerenv ]] && return 0
  if [[ -r /proc/1/cgroup ]] && grep -Eiq '(docker|containerd|kubepods|podman|lxc)' /proc/1/cgroup; then
    return 0
  fi
  return 1
}

select_linux_build_method() {
  if [[ ! "$BUILD_TARGET" =~ -unknown-linux ]]; then
    return 0
  fi

  if linux_running_in_container; then
    case "$BUILD_METHOD" in
      auto)
        BUILD_METHOD="local"
        echo "  Detected container environment, auto-selecting local build."
        return 0
        ;;
      docker)
        echo "Error: BUILD_METHOD=docker is not supported when build.sh is already running inside a container." >&2
        echo "Use BUILD_METHOD=local or BUILD_METHOD=remote instead." >&2
        exit 1
        ;;
      local|remote)
        return 0
        ;;
      *)
        echo "Error: BUILD_METHOD must be one of: auto, docker, local, remote" >&2
        exit 1
        ;;
    esac
  fi

  case "$BUILD_METHOD" in
    local|docker|remote) return 0 ;;
    auto) ;;
    *)
      echo "Error: BUILD_METHOD must be one of: auto, docker, local, remote" >&2
      exit 1
      ;;
  esac

  if [[ ! -t 0 ]]; then
    return 0
  fi

  local default_choice="1"
  if [[ "$(uname -s)" == "Darwin" ]] && command -v docker &>/dev/null && docker info &>/dev/null; then
    default_choice="2"
  fi

  echo ""
  echo "========== Linux Build Method =========="
  echo "  1) Local build on this machine"
  echo "  2) Docker build on this machine"
  echo "  3) Remote build over SSH"
  echo ""
  read -r -p "Select method [1-3] (default ${default_choice}): " build_choice
  build_choice=${build_choice:-$default_choice}
  case "$build_choice" in
    1) BUILD_METHOD="local" ;;
    2) BUILD_METHOD="docker" ;;
    3) BUILD_METHOD="remote" ;;
    *)
      echo "Error: invalid Linux build method: $build_choice" >&2
      exit 1
      ;;
  esac
}

build_target_override_from_args() {
  local i arg
  for (( i=0; i < ${#BUILD_ARGS[@]}; i++ )); do
    arg="${BUILD_ARGS[$i]}"
    case "$arg" in
      --target)
        if (( i + 1 < ${#BUILD_ARGS[@]} )); then
          printf '%s\n' "${BUILD_ARGS[$((i + 1))]}"
          return 0
        fi
        ;;
      --target=*)
        printf '%s\n' "${arg#--target=}"
        return 0
        ;;
    esac
  done
  return 1
}

platform_choice_from_target() {
  local target="$1"
  case "$target" in
    xtensa-*-espidf) printf '%s\n' '1' ;;
    riscv32imafc-esp-espidf) printf '%s\n' '2' ;;
    x86_64-unknown-linux-*) printf '%s\n' '3' ;;
    armv7-unknown-linux-*) printf '%s\n' '4' ;;
    aarch64-unknown-linux-*) printf '%s\n' '5' ;;
    *) return 1 ;;
  esac
}

select_build_platform() {
  # If TARGET env is set, skip interactive prompt.
  if [[ -n "${TARGET:-}" ]]; then
    case "${TARGET}" in
      esp|esp32|esp32s3) PLATFORM_CHOICE=1; return 0 ;;
      p4|esp32p4) PLATFORM_CHOICE=2; return 0 ;;
      linux) PLATFORM_CHOICE=3; return 0 ;;
      linux-armv7|armv7) PLATFORM_CHOICE=4; return 0 ;;
      linux-aarch64|aarch64) PLATFORM_CHOICE=5; return 0 ;;
      *) echo "Error: Unknown TARGET=$TARGET. Use 'esp', 'esp32s3', 'p4', 'linux', 'linux-armv7', or 'linux-aarch64'" >&2; exit 1 ;;
    esac
  fi

  if [[ -n "${BOARD:-}" ]]; then
    case "${BOARD}" in
      esp32-p4-*) PLATFORM_CHOICE=2 ;;
      *) PLATFORM_CHOICE=1 ;;
    esac
    return 0
  fi

  local build_target_override=""
  build_target_override="$(build_target_override_from_args || true)"
  if [[ -n "$build_target_override" ]]; then
    PLATFORM_CHOICE="$(platform_choice_from_target "$build_target_override")" || {
      echo "Error: unsupported --target for platform auto-selection: $build_target_override" >&2
      exit 1
    }
    return 0
  fi

  # If --flash / --flash-update is set, default to ESP32-S3.
  if [[ -n "$DO_FLASH" ]]; then
    PLATFORM_CHOICE=1
    return 0
  fi

  echo ""
  echo "=========================================="
  echo "  $MSG_TITLE"
  echo "=========================================="
  echo ""
  echo "$MSG_SELECT_PLATFORM"
  echo "  1) $MSG_PLATFORM_ESP_S3"
  echo "  2) $MSG_PLATFORM_ESP_P4"
  echo "  3) $MSG_PLATFORM_LINUX"
  echo "  4) $MSG_PLATFORM_LINUX_ARMV7"
  echo "  5) $MSG_PLATFORM_LINUX_AARCH64"
  echo ""

  while true; do
    read -r -p "$MSG_INPUT_OPTION [1-5] ($MSG_PRESS_ENTER 1): " choice
    choice=${choice:-1}
    case "$choice" in
      1) PLATFORM_CHOICE=1; return 0 ;;
      2) PLATFORM_CHOICE=2; return 0 ;;
      3) PLATFORM_CHOICE=3; return 0 ;;
      4) PLATFORM_CHOICE=4; return 0 ;;
      5) PLATFORM_CHOICE=5; return 0 ;;
      *) echo "$MSG_INVALID_OPTION 1, 2, 3, 4, or 5" ;;
    esac
  done
}

# Execute platform selection.
PLATFORM_CHOICE=1
select_build_platform

if [[ $PLATFORM_CHOICE -eq 3 || $PLATFORM_CHOICE -eq 4 || $PLATFORM_CHOICE -eq 5 ]]; then
  # Linux 构建
  echo ""
  echo "========== $MSG_LINUX_MODE =========="
  if [[ $PLATFORM_CHOICE -eq 4 ]]; then
    BUILD_TARGET="armv7-unknown-linux-musleabihf"
  elif [[ $PLATFORM_CHOICE -eq 5 ]]; then
    BUILD_TARGET="aarch64-unknown-linux-musl"
  fi

  # 检测当前系统
  CURRENT_OS="$(uname -s)"
  if [[ "$CURRENT_OS" == "Linux" ]]; then
    # Linux 本机上，若当前宿主与目标架构一致，则优先使用原生 GNU target；
    # musl 主要用于 macOS/host 交叉产物与非原生 Linux 交叉路线。
    CURRENT_ARCH="$(uname -m)"
    if [[ $PLATFORM_CHOICE -eq 3 ]]; then
      BUILD_TARGET="x86_64-unknown-linux-gnu"
    elif [[ $PLATFORM_CHOICE -eq 4 ]] && [[ "$CURRENT_ARCH" == "armv7l" || "$CURRENT_ARCH" == "armv6l" ]]; then
      BUILD_TARGET="armv7-unknown-linux-gnueabihf"
    elif [[ $PLATFORM_CHOICE -eq 5 ]] && [[ "$CURRENT_ARCH" == "aarch64" || "$CURRENT_ARCH" == "arm64" ]]; then
      BUILD_TARGET="aarch64-unknown-linux-gnu"
    fi
    echo "  $MSG_DETECTED_LINUX"
  elif [[ "$CURRENT_OS" == "Darwin" ]]; then
    # On macOS, cross-compile to Linux.
    echo "  $MSG_DETECTED_MACOS"
    if [[ $PLATFORM_CHOICE -eq 3 ]]; then
      BUILD_TARGET="x86_64-unknown-linux-musl"
      LOCAL_LINKER_CMD="x86_64-linux-musl-gcc"
    elif [[ $PLATFORM_CHOICE -eq 4 ]]; then
      BUILD_TARGET="armv7-unknown-linux-musleabihf"
      LOCAL_LINKER_CMD="arm-linux-musleabihf-gcc"
    else
      BUILD_TARGET="aarch64-unknown-linux-musl"
      LOCAL_LINKER_CMD="aarch64-linux-musl-gcc"
    fi

  else
    echo "$MSG_UNKNOWN_OS $CURRENT_OS, $MSG_TRY_NATIVE"
    BUILD_TARGET="x86_64-unknown-linux-gnu"
  fi

  select_linux_build_method

  if [[ "$BUILD_METHOD" == "docker" ]]; then
    linux_apply_docker_target_for_platform
  fi

  if [[ "$BUILD_METHOD" == "remote" ]]; then
    linux_prepare_remote_build_context
  elif [[ "$CURRENT_OS" == "Darwin" ]]; then
    # Docker CLI alone is not enough (Desktop may be off); require a running daemon for auto/local mode.
    HAS_DOCKER_CLI=0
    command -v docker &>/dev/null && HAS_DOCKER_CLI=1
    HAS_DOCKER_DAEMON=0
    if [[ $HAS_DOCKER_CLI -eq 1 ]] && docker info &>/dev/null; then
      HAS_DOCKER_DAEMON=1
    fi

    case "$BUILD_METHOD" in
      docker)
        [[ $HAS_DOCKER_CLI -eq 1 ]] || {
          echo "$MSG_ERROR_NO_DOCKER"
          echo "$MSG_INSTALL_DOCKER: https://www.docker.com/products/docker-desktop"
          exit 1
        }
        docker info &>/dev/null || {
          echo "Error: Docker is installed but the daemon is not running." >&2
          echo "Start Docker Desktop, or use: BUILD_METHOD=local ./build.sh" >&2
          exit 1
        }
        USE_DOCKER=1
        ;;
      local)
        USE_DOCKER=""
        ;;
      auto)
        # Prefer Docker only when the daemon responds (avoids broken socket when Desktop is off).
        if [[ $HAS_DOCKER_DAEMON -eq 1 ]]; then
          USE_DOCKER=1
          linux_apply_docker_target_for_platform
          echo "  Auto-selected Docker build (daemon reachable)."
        else
          USE_DOCKER=""
          if [[ $HAS_DOCKER_CLI -eq 1 ]]; then
            echo "  Auto-selected local musl-cross build (Docker daemon not running)."
          else
            echo "  Auto-selected local musl-cross build (Docker not in PATH)."
          fi
        fi
        ;;
      *)
        echo "Error: BUILD_METHOD must be one of: auto, docker, local, remote" >&2
        exit 1
        ;;
    esac
  elif [[ "$CURRENT_OS" == "Linux" ]]; then
    case "$BUILD_METHOD" in
      docker) USE_DOCKER=1 ;;
      local|auto) USE_DOCKER="" ;;
      *)
        echo "Error: BUILD_METHOD must be one of: auto, docker, local, remote" >&2
        exit 1
        ;;
    esac
  fi

  BUILD_FEATURES=""
  SKIP_ESP_TOOLCHAIN=1
else
  # ESP32 构建
  echo ""
  echo "========== $MSG_ESP_MODE =========="

  case "$PLATFORM_CHOICE" in
    2) BUILD_TARGET="riscv32imafc-esp-espidf" ;;
    *) BUILD_TARGET="xtensa-esp32s3-espidf" ;;
  esac
  BUILD_FEATURES=""
  BUILD_PROFILE="release-size"
fi
CLI_BUILD_TARGET=""
for (( i=0; i < ${#BUILD_ARGS[@]}; i++ )); do
  case "${BUILD_ARGS[$i]}" in
    --target)
      if (( i + 1 < ${#BUILD_ARGS[@]} )); then
        CLI_BUILD_TARGET="${BUILD_ARGS[$i+1]}"
        break
      fi
      ;;
    --target=*)
      CLI_BUILD_TARGET="${BUILD_ARGS[$i]#--target=}"
      break
      ;;
  esac
done
if [[ -n "${BOARD:-}" ]]; then
  if [[ ! "$BOARD" =~ ^[a-z0-9-]+$ ]]; then
    echo "Error: BOARD must contain only [a-z0-9-]. Got: $BOARD" >&2
    exit 1
  fi
  PRESETS_FILE="$SCRIPT_ROOT/board_presets.toml"
  if [[ ! -f "$PRESETS_FILE" ]]; then
    echo "Error: BOARD=$BOARD set but board_presets.toml not found" >&2
    exit 1
  fi
  section="[boards.$BOARD]"
  block=$(awk -v section="$section" '
    $0 == section { found=1; next }
    found { print }
    /^\[/ && found { exit }
  ' "$PRESETS_FILE")
  if [[ -z "$block" ]]; then
    echo "Error: Unknown board: $BOARD" >&2
    echo "Known boards: $(grep -E '^\[boards\.' "$PRESETS_FILE" 2>/dev/null | sed 's/\[boards\.\(.*\)\]/\1/' | tr '\n' ' ')" >&2
    exit 1
  fi
  BUILD_TARGET=$(echo "$block" | grep -E '^target\s*=' | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
  [[ -z "$BUILD_TARGET" ]] && { echo "Error: board $BOARD has no 'target' in board_presets.toml" >&2; exit 1; }
  PARTITION_TABLE=$(echo "$block" | grep -E '^partition_table\s*=' | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
  BOARD_SDKCONFIG_OVERLAY=$(echo "$block" | grep -E '^sdkconfig_overlay\s*=' | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
  if [[ -z "$PACKAGE_PROFILE" ]]; then
    PACKAGE_PROFILE=$(echo "$block" | grep -E '^package_profile\s*=' | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
  fi
  if [[ -z "$PARTITION_TABLE" ]]; then
    case "$BOARD" in
      esp32-s3-8mb)  PARTITION_TABLE=partitions_8mb.csv ;;
      esp32-s3-32mb) PARTITION_TABLE=partitions_32mb.csv ;;
      *)             PARTITION_TABLE=partitions.csv ;;
    esac
  fi
else
  PARTITION_TABLE=partitions.csv
  BOARD_SDKCONFIG_OVERLAY=""
fi
# Command-line --target overrides BOARD (same as build.ps1)
if [[ -n "$CLI_BUILD_TARGET" ]]; then
  BUILD_TARGET="$CLI_BUILD_TARGET"
fi
# Sanitize target (no path chars)
if [[ ! "$BUILD_TARGET" =~ ^[a-zA-Z0-9_-]+$ ]]; then
  echo "Error: Invalid --target (no path chars): $BUILD_TARGET" >&2
  exit 1
fi

TARGET_MCU="$(target_mcu_from_triple "$BUILD_TARGET" || true)"
if [[ -z "$TARGET_MCU" && ! "$BUILD_TARGET" =~ -unknown-linux ]]; then
  echo "Error: unsupported ESP target triple: $BUILD_TARGET" >&2
  exit 1
fi
if [[ -z "${BOARD_SDKCONFIG_OVERLAY:-}" && ! "$BUILD_TARGET" =~ -unknown-linux ]]; then
  BOARD_SDKCONFIG_OVERLAY="$(default_sdkconfig_overlay_for_target "$BUILD_TARGET" || true)"
fi
if [[ -n "${BOARD_SDKCONFIG_OVERLAY:-}" && ! -f "$SCRIPT_ROOT/$BOARD_SDKCONFIG_OVERLAY" ]]; then
  echo "Error: sdkconfig overlay not found: $BOARD_SDKCONFIG_OVERLAY" >&2
  exit 1
fi

if [[ -z "$PACKAGE_PROFILE" ]]; then
  PACKAGE_PROFILE="$(default_package_profile_for_target "$BUILD_TARGET")"
fi
if [[ ! "$PACKAGE_PROFILE" =~ ^[a-z0-9+_-]+$ ]]; then
  echo "Error: invalid package profile: $PACKAGE_PROFILE" >&2
  exit 1
fi
BUILD_FEATURES="$(package_profile_features "$PACKAGE_PROFILE")"
export BEETLE_PACKAGE_PROFILE="$PACKAGE_PROFILE"

# Derive chip from target for flash (same as build.ps1)
FLASH_CHIP="${TARGET_MCU:-}"

# Print detected hardware / build config (same as build.ps1 Write-BuildStatus)
echo ""
echo "========== Detected hardware / build config =========="
echo "  Project root:      $SCRIPT_ROOT"
echo "  Build target:      $BUILD_TARGET"
echo "  BOARD (optional):  ${BOARD:-(not set)}"
echo "  Partition table:   $PARTITION_TABLE"
echo "  Target MCU:        ${TARGET_MCU:-(N/A)}"
echo "  SDKCONFIG overlay: ${BOARD_SDKCONFIG_OVERLAY:-(none)}"
echo "  Chip (for flash):  ${FLASH_CHIP:-(N/A)}"
echo "  Package profile:   ${PACKAGE_PROFILE:-(none)}"
echo "  Features:          ${BUILD_FEATURES:-(none)}"
echo "  Profile:           $BUILD_PROFILE"
echo ""

# --- clean: cargo clean then exit (no short-path on Mac/Linux) ---
if printf '%s\n' "${BUILD_ARGS[@]}" | grep -qx "clean"; then
  echo "========== Step: Cleaning build artifacts =========="
  echo "  Running: cargo clean (project root)..."
  ensure_local_stable_toolchain
  CLEAN_ARGS=()
  for a in "${BUILD_ARGS[@]}"; do [[ "$a" != "clean" ]] && CLEAN_ARGS+=("$a"); done
  cargo clean "${CLEAN_ARGS[@]}"
  exit $?
fi

# effectiveTargetDir (same as build.ps1)
EFFECTIVE_TARGET_DIR="${CARGO_TARGET_DIR:-$SCRIPT_ROOT/target}"
RELEASE_DIR="$EFFECTIVE_TARGET_DIR/$BUILD_TARGET/$BUILD_PROFILE"
BIN="$RELEASE_DIR/beetle"
if [[ "$BUILD_METHOD" == "remote" ]] && [[ -n "${REMOTE_BUILD_DIR:-}" ]]; then
  REMOTE_BUILD_BIN="$REMOTE_BUILD_DIR/target/$BUILD_TARGET/$BUILD_PROFILE/beetle"
fi
BOOTLOADER_BIN="$RELEASE_DIR/bootloader.bin"
PARTITION_TABLE_BIN="$RELEASE_DIR/partition-table.bin"
PARTITION_CSV="$SCRIPT_ROOT/$PARTITION_TABLE"
ESP_IDF_BUILD_DIR="$(find "$RELEASE_DIR/build" -path '*/out/build' -type d 2>/dev/null | head -n 1)"
APP_BIN="$RELEASE_DIR/beetle.bin"
OTADATA_BIN=""
FLASHER_ARGS_JSON=""
APP_FLASH_MODE=""
APP_FLASH_SIZE=""
APP_FLASH_FREQ=""
if [[ -n "$ESP_IDF_BUILD_DIR" ]]; then
  OTADATA_BIN="$ESP_IDF_BUILD_DIR/ota_data_initial.bin"
  FLASHER_ARGS_JSON="$ESP_IDF_BUILD_DIR/flasher_args.json"
  if [[ -f "$FLASHER_ARGS_JSON" ]]; then
    APP_FLASH_MODE="$(sed -n 's/.*"flash_mode":[[:space:]]*"\([^"]*\)".*/\1/p' "$FLASHER_ARGS_JSON" | head -n1)"
    APP_FLASH_SIZE="$(sed -n 's/.*"flash_size":[[:space:]]*"\([^"]*\)".*/\1/p' "$FLASHER_ARGS_JSON" | head -n1)"
    APP_FLASH_FREQ="$(sed -n 's/.*"flash_freq":[[:space:]]*"\([^"]*\)".*/\1/p' "$FLASHER_ARGS_JSON" | head -n1)"
  fi
fi
if [[ -n "$DO_FLASH" ]] && [[ ! "$BUILD_TARGET" =~ -unknown-linux ]] && [[ -z "$FLASH_CHIP" ]]; then
  echo "Error: Cannot derive chip from target for flash: $BUILD_TARGET" >&2
  exit 1
fi

# --- Flash mode: numbered menu (same style as Linux deploy mode menu; sets ERASE_BEFORE_FLASH; may exit) ---
# FLASH_NO_ERASE=1 (--flash-update): skip menu, never erase.
select_flash_mode() {
  local port="$1" triple="$2"
  ERASE_BEFORE_FLASH=0
  if [[ -n "$FLASH_NO_ERASE" ]]; then
    echo -e "${GREEN}✓ Flash mode: update only — entire flash will NOT be erased (NVS / config preserved).${NC}"
    echo ""
    return 0
  fi
  echo "========== Flash mode =========="
  echo ""
  echo "  1) Update flash — keep NVS, WiFi credentials, SPIFFS (typical dev / OTA-style)"
  echo "  2) Full chip erase then flash — wipes entire flash (factory reset / partition change)"
  echo "  3) Cancel"
  echo ""
  while true; do
    read -r -p "Select [1-3] (default 1): " flash_choice
    flash_choice=${flash_choice:-1}
    case "$flash_choice" in
      1)
        ERASE_BEFORE_FLASH=0
        echo -e "${GREEN}✓ Update flash: no full erase.${NC}"
        echo ""
        return 0
        ;;
      2)
        echo -e "${YELLOW}⚠ Entire flash will be erased on ${port}; firmware target: ${triple}${NC}"
        read -r -p "Type 'yes' to confirm full erase and flash: " confirm
        if [[ "$confirm" != "yes" ]]; then
          echo "Aborted."
          exit 0
        fi
        ERASE_BEFORE_FLASH=1
        echo ""
        return 0
        ;;
      3)
        echo "Cancelled."
        exit 0
        ;;
      *)
        echo -e "${YELLOW}Invalid option — enter 1, 2, or 3${NC}"
        ;;
    esac
  done
}

# Port validation: /dev/ path only (Mac/Linux)
valid_flash_port() {
  [[ -n "$1" ]] && [[ "$1" =~ ^/dev/[a-zA-Z0-9/_.-]+$ ]] && [[ "$1" != *".."* ]]
}

collect_esp_component_graph_inputs() {
  local path
  for path in \
    Cargo.toml \
    build.rs \
    components_esp32s3.lock \
    components_esp32p4.lock \
    sdkconfig.defaults \
    sdkconfig.defaults.esp32s3 \
    sdkconfig.defaults.esp32s3.8mb.board \
    sdkconfig.defaults.esp32s3.board \
    sdkconfig.defaults.esp32s3.32mb.board \
    sdkconfig.defaults.esp32p4 \
    sdkconfig.defaults.esp32p4.board \
    third_party/esp-idf-sys/build/native/cargo_driver/config.rs
  do
    [[ -f "$SCRIPT_ROOT/$path" ]] && printf '%s\n' "$SCRIPT_ROOT/$path"
  done
  if [[ -d "$SCRIPT_ROOT/components" ]]; then
    find "$SCRIPT_ROOT/components" -type f ! -name '.DS_Store' | sort
  fi
}

compute_esp_component_graph_hash() {
  local hasher=()
  local file

  if command -v shasum >/dev/null 2>&1; then
    hasher=(shasum -a 256)
  elif command -v sha256sum >/dev/null 2>&1; then
    hasher=(sha256sum)
  else
    echo "Error: need shasum or sha256sum to hash ESP component graph inputs" >&2
    return 1
  fi

  while IFS= read -r file; do
    [[ -n "$file" ]] || continue
    "${hasher[@]}" "$file"
  done < <(collect_esp_component_graph_inputs) | "${hasher[@]}" | awk '{print $1}'
}

refresh_esp_component_graph_cache() {
  [[ "$BUILD_TARGET" =~ -unknown-linux ]] && return 0

  local stamp_dir="$EFFECTIVE_TARGET_DIR/$BUILD_TARGET/$BUILD_PROFILE"
  local stamp_file="$stamp_dir/.beetle-esp-component-graph.sha256"
  local current_hash cached_hash=""

  current_hash="$(compute_esp_component_graph_hash)" || return 1
  [[ -f "$stamp_file" ]] && cached_hash="$(tr -d '[:space:]' < "$stamp_file")"

  if [[ "$current_hash" == "$cached_hash" ]]; then
    return 0
  fi

  mkdir -p "$stamp_dir"
  if [[ -d "$stamp_dir/build" ]]; then
    find "$stamp_dir/build" -maxdepth 1 -type d -name 'esp-idf-sys-*' -exec rm -rf {} +
  fi
  printf '%s\n' "$current_hash" > "$stamp_file"
}

# macOS/Linux: show who holds the serial device (common cause of espflash "Failed to open serial port").
warn_serial_port_busy() {
  local p="$1"
  if [[ -z "$p" ]]; then
    return 0
  fi
  if [[ ! -e "$p" ]]; then
    echo -e "${YELLOW}  Serial device not found: $p (cable unplugged or USB re-enumerated — replug and pick port again).${NC}" >&2
    return 0
  fi
  command -v lsof &>/dev/null || {
    echo "  (Install lsof to see which process holds the serial port.)" >&2
    return 0
  }
  local devs=("$p")
  if [[ "$(uname -s)" = "Darwin" ]] && [[ "$p" == /dev/cu.* ]]; then
    devs+=("${p/\/dev\/cu./\/dev\/tty.}")
  fi
  local found=0
  for d in "${devs[@]}"; do
    [[ -e "$d" ]] || continue
    local out
    out=$(lsof "$d" 2>/dev/null || true)
    if [[ -n "$out" ]]; then
      echo -e "${YELLOW}  Another process is using $d — quit it, then flash again:${NC}" >&2
      echo "$out" >&2
      found=1
    fi
  done
  if [[ $found -eq 0 ]]; then
    echo "  lsof: no process holds $p (or tty sibling)." >&2
  fi
}

print_flash_open_port_hints() {
  echo "" >&2
  echo -e "${RED}Device connection failed.${NC}" >&2
  echo "  Beetle already retried the built-in connection methods for this board." >&2
  echo "  Next steps:" >&2
  echo "    1) Unplug and replug the board, then run ./build.sh again." >&2
  echo "    2) Close any serial monitor or IDE that might still hold the device." >&2
  echo "    3) If the board has a BOOT button, hold BOOT, tap RESET, then retry immediately." >&2
  echo "    4) On macOS, try a direct cable/port instead of a hub." >&2
  warn_serial_port_busy "${CHOSEN_PORT:-}"
}
# Ensure espflash installed (same as build.ps1 Ensure-Espflash)
ensure_espflash() {
  if command -v espflash &>/dev/null; then return; fi
  echo ""
  echo "========== Step: Ensuring espflash is installed =========="
  echo "  espflash not found. Running: cargo install espflash"
  RUSTUP_TOOLCHAIN=stable cargo install espflash
  export PATH="${HOME}/.cargo/bin:${PATH}"
  command -v espflash &>/dev/null || { echo "Error: espflash install failed." >&2; exit 1; }
}

generate_app_bin_from_elf() {
  [[ -f "$BIN" ]] || return 1
  local flash_mode="${APP_FLASH_MODE:-dio}"
  local flash_size="${APP_FLASH_SIZE:-16MB}"
  local flash_freq="${APP_FLASH_FREQ:-80m}"
  if ! python3 -m esptool --chip "$FLASH_CHIP" elf2image \
      --flash-mode "$flash_mode" \
      --flash-size "$flash_size" \
      --flash-freq "$flash_freq" \
      -o "$APP_BIN" \
      "$BIN" >/dev/null; then
    echo "Error: failed to generate app bin from ELF: $BIN" >&2
    return 1
  fi
}
# Interactive port selection when ESPFLASH_PORT not set (same as build.ps1 Get-FlashPort)
# Only the chosen port is printed to stdout; messages go to stderr.
get_flash_port() {
  if [[ -n "${ESPFLASH_PORT:-}" ]]; then
    valid_flash_port "$ESPFLASH_PORT" || { echo "Error: ESPFLASH_PORT must be a valid device path (e.g. /dev/ttyUSB0). Got: $ESPFLASH_PORT" >&2; exit 1; }
    echo "$ESPFLASH_PORT"
    return
  fi
  PORTS=()
  local port
  while IFS= read -r port; do
    [[ -n "$port" ]] && PORTS+=("$port")
  done < <(list_flash_ports)
  if [[ ${#PORTS[@]} -eq 0 ]]; then
    echo "No serial ports found. Plug in the board or set ESPFLASH_PORT=/dev/..." >&2
    exit 1
  fi
  if [[ ${#PORTS[@]} -eq 1 ]]; then
    echo "  Detected 1 serial port: ${PORTS[0]}" >&2
    echo "${PORTS[0]}"
    return
  fi
  local preferred_port=""
  preferred_port="$(beetle_preferred_flash_port_for_chip "$FLASH_CHIP" "${PORTS[@]}" || true)"
  if [[ -n "$preferred_port" ]]; then
    echo "  Detected ${#PORTS[@]} serial ports; auto-selected ${preferred_port} for ${FLASH_CHIP}." >&2
    echo "$preferred_port"
    return
  fi
  echo "  Detected ${#PORTS[@]} serial ports. Select port to flash (ESP board):" >&2
  for i in "${!PORTS[@]}"; do echo "  $((i+1)). ${PORTS[i]}" >&2; done
  if [[ "$(uname -s)" != "Linux" ]]; then
    if [[ "$FLASH_CHIP" == "esp32p4" ]]; then
      echo -e "${YELLOW}  Tip: For ESP32-P4, prefer the USB-UART serial port when both a native USB alias and a UART bridge are visible.${NC}" >&2
    else
      echo -e "${YELLOW}  Tip: Prefer a cu.usbmodem* entry for ESP32-S3 native USB; if you see wchusbserial with the same ID as usbmodem, avoid the duplicate — pick the other.${NC}" >&2
    fi
  fi
  while true; do
    read -r -p "Enter number (1-${#PORTS[@]}): " sel
    if [[ "$sel" =~ ^[0-9]+$ ]] && (( sel >= 1 && sel <= ${#PORTS[@]} )); then
      echo "${PORTS[$((sel-1))]}"
      return
    fi
    echo "Invalid, enter 1-${#PORTS[@]}" >&2
  done
}

get_model_partition_offset() {
  [[ -f "$PARTITION_CSV" ]] || return 0
  awk -F',' '
    function trim(s) {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", s)
      return s
    }
    $0 !~ /^[[:space:]]*#/ && NF >= 4 {
      name = trim($1)
      offset = trim($4)
      if (name == "model") {
        print offset
        exit
      }
    }
  ' "$PARTITION_CSV"
}

find_srmodels_bin() {
  local matches=()
  shopt -s nullglob
  matches=("$RELEASE_DIR"/build/esp-idf-sys-*/out/build/srmodels/srmodels.bin)
  shopt -u nullglob
  if (( ${#matches[@]} == 0 )); then
    return 1
  fi
  ls -1t "${matches[@]}" 2>/dev/null | head -n1
}

file_md5_hex() {
  local file="$1"
  if command -v md5 >/dev/null 2>&1; then
    md5 -q "$file" | tr '[:upper:]' '[:lower:]'
    return 0
  fi
  if command -v md5sum >/dev/null 2>&1; then
    md5sum "$file" | awk '{print tolower($1)}'
    return 0
  fi
  if command -v openssl >/dev/null 2>&1; then
    openssl dgst -md5 -r "$file" | awk '{print tolower($1)}'
    return 0
  fi
  return 1
}

device_region_md5_hex() {
  local address="$1" size="$2" output
  if ! output="$(run_espflash_with_connection_profiles checksum-md5 --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" "$address" "$size" 2>&1)"; then
    return 1
  fi
  printf '%s\n' "$output" | grep -Eio '[0-9a-f]{32}' | tail -n1 | tr '[:upper:]' '[:lower:]'
}

run_espflash_with_connection_profiles() {
  local subcommand="$1"
  shift

  local profile last_status=1 status=1
  local attempt=0
  local profile_args=()
  while IFS= read -r profile; do
    [[ -n "$profile" ]] || continue
    attempt=$((attempt + 1))
    read -r -a profile_args <<< "$profile"
    if [[ $attempt -gt 1 ]]; then
      echo "  Retrying device connection..." >&2
    fi
    if espflash "$subcommand" "${profile_args[@]}" "$@"; then
      return 0
    else
      status=$?
    fi
    last_status=$status
  done < <(beetle_espflash_connection_profiles "$FLASH_CHIP" "$subcommand")
  return "$last_status"
}

flash_model_partition_if_present() {
  local model_offset model_bin model_size local_md5 device_md5
  model_offset="$(get_model_partition_offset)"
  if [[ -z "$model_offset" ]]; then
    echo "  Model partition:   not present in $PARTITION_TABLE (wake-word model flash skipped)"
    return 0
  fi

  if ! model_bin="$(find_srmodels_bin)"; then
    echo -e "${RED}Error: model partition exists but srmodels.bin was not generated.${NC}" >&2
    echo "  Expected under: $RELEASE_DIR/build/esp-idf-sys-*/out/build/srmodels/srmodels.bin" >&2
    return 1
  fi

  echo ""
  echo "========== Flashing wake-word model =========="
  echo ""
  echo "  Model image:  $model_bin"
  echo "  Model offset: $model_offset"
  if [[ "${ERASE_BEFORE_FLASH:-0}" -ne 1 ]]; then
    model_size="$(wc -c < "$model_bin" | tr -d '[:space:]')"
    if local_md5="$(file_md5_hex "$model_bin")" && [[ -n "$local_md5" ]]; then
      if device_md5="$(device_region_md5_hex "$model_offset" "$model_size")" && [[ -n "$device_md5" ]]; then
        echo "  Model MD5(local):  $local_md5"
        echo "  Model MD5(device): $device_md5"
        if [[ "$local_md5" == "$device_md5" ]]; then
          echo -e "${GREEN}✓ Wake-word model unchanged; skipping model flash.${NC}"
          return 0
        fi
      else
        echo "  Model MD5(device): unavailable; model will be reflashed"
      fi
    else
      echo "  Model MD5(local):  unavailable; model will be reflashed"
    fi
  fi
  if ! run_espflash_with_connection_profiles write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" "$model_offset" "$model_bin"; then
    echo "" >&2
    echo -e "${RED}Wake-word model flash failed.${NC}" >&2
    return 1
  fi
  echo -e "${GREEN}✓ Wake-word model flashed.${NC}"
  return 0
}

open_monitor_if_requested() {
  [[ -n "$NO_MONITOR" ]] && return 0
  echo ""
  echo "========== Opening serial monitor =========="
  echo ""
  if ! espflash reset --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" --before no-reset --after hard-reset; then
    print_flash_open_port_hints
    return 1
  fi
  sleep 1
  local monitor_baud="${MONITOR_BAUD:-115200}"
  if python3 - <<'PY' >/dev/null 2>&1
import importlib.util, sys
sys.exit(0 if importlib.util.find_spec("serial.tools.miniterm") else 1)
PY
  then
    python3 -m serial.tools.miniterm "$CHOSEN_PORT" "$monitor_baud"
    return $?
  fi
  if command -v screen >/dev/null 2>&1; then
    screen "$CHOSEN_PORT" "$monitor_baud"
    return $?
  fi
  echo "  Monitor tool not available. Install pyserial or screen, or rerun with --no-monitor." >&2
  return 1
}

# ESP: full flash workflow (shared by --flash and interactive "deploy yes").
run_esp_flash_workflow() {
  if [[ ! -f "$BIN" ]]; then
    echo "Error: Binary not found: $BIN" >&2
    return 1
  fi
  if [[ -z "$FLASH_CHIP" ]]; then
    echo "Error: Cannot derive chip from target for flash: $BUILD_TARGET" >&2
    return 1
  fi
  FLASH_EXTRA=()
  PARTITION_FOR_FLASH="$PARTITION_CSV"
  if [[ -f "$BOOTLOADER_BIN" ]]; then
    [[ -f "$PARTITION_TABLE_BIN" ]] && PARTITION_FOR_FLASH="$PARTITION_TABLE_BIN"
    if [[ -f "$PARTITION_FOR_FLASH" ]]; then
      FLASH_EXTRA=(--bootloader "$BOOTLOADER_BIN" --partition-table "$PARTITION_FOR_FLASH")
    fi
  fi

  ensure_espflash
  if ! CHOSEN_PORT="$(get_flash_port)"; then
    return 1
  fi
  echo ""
  echo "=========================================="
  echo "  Beetle — Flash to device"
  echo "=========================================="
  echo ""
  echo "========== Flash: hardware and paths =========="
  echo ""
  echo "  Project root:      $SCRIPT_ROOT"
  echo "  Build target:      $BUILD_TARGET"
  echo "  BOARD (optional):  ${BOARD:-(not set)}"
  echo "  Chip (for flash):  ${FLASH_CHIP:-(N/A)}"
  echo "  Package profile:   ${PACKAGE_PROFILE:-(none)}"
  echo "  Features:          ${BUILD_FEATURES:-(none)}"
  echo -e "  ${BLUE}Serial port:${NC}       $CHOSEN_PORT"
  echo "  Partition table:   $PARTITION_FOR_FLASH"
  echo "  Bootloader:        $BOOTLOADER_BIN"
  echo "  Firmware ELF:      $BIN"
  echo "  Firmware app bin:  ${APP_BIN:-"(not found)"}"
  echo ""

  echo "========== Checking connection =========="
  echo ""
  echo "  Serial port occupancy (lsof):" >&2
  warn_serial_port_busy "$CHOSEN_PORT"
  echo ""
  if run_espflash_with_connection_profiles board-info --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 2>/dev/null; then
    echo -e "${GREEN}✓ board-info OK${NC}"
  else
    echo -e "${YELLOW}⚠ Device check did not complete before flashing.${NC}"
    echo "  Beetle will continue and use its built-in connection retries for this board."
  fi
  echo ""

  select_flash_mode "$CHOSEN_PORT" "$BUILD_TARGET"

  if [[ "$ERASE_BEFORE_FLASH" -eq 1 ]]; then
    echo "========== Erasing entire flash =========="
    echo ""
    echo "  Port: $CHOSEN_PORT  |  Chip: $FLASH_CHIP"
    local full_erase_transport=""
    full_erase_transport="$(beetle_full_erase_transport_for_chip "$FLASH_CHIP")"
    if [[ "$full_erase_transport" == "esptool" ]]; then
      if ! python3 -m esptool --chip "$FLASH_CHIP" --port "$CHOSEN_PORT" --before default-reset --after no-reset erase-flash; then
        echo "" >&2
        echo -e "${RED}Erase failed.${NC}" >&2
        echo "  Beetle could not complete the full erase on this device." >&2
        echo "  Try reconnecting the board and running ./build.sh again." >&2
        return 1
      fi
    elif ! run_espflash_with_connection_profiles erase-flash --port "$CHOSEN_PORT" --chip "$FLASH_CHIP"; then
      echo "" >&2
      echo -e "${RED}Erase failed.${NC}" >&2
      echo "  Beetle could not reconnect to the device after trying its built-in connection methods." >&2
      echo "  Try reconnecting the board and running ./build.sh again." >&2
      return 1
    fi
    echo -e "${GREEN}✓ Erase completed. Waiting 2s before flash.${NC}"
    sleep 2
  else
    echo "========== Skipping full erase (update flash) =========="
    echo ""
    echo -e "  ${GREEN}✓ NVS and other flash regions are left unchanged.${NC}"
    echo "  Port: $CHOSEN_PORT  |  Chip: $FLASH_CHIP"
  fi

  echo ""
  echo "========== Flashing firmware =========="
  echo ""
  if [[ ! -f "$APP_BIN" ]]; then
    echo "Error: app bin not found: ${APP_BIN:-<empty>}" >&2
    echo "Expected generated app bin from ELF: $BIN" >&2
    return 1
  fi
  echo "  ELF: $BIN"
  echo "  App bin: $APP_BIN"
  echo "  Partition table: $PARTITION_FOR_FLASH"

  if [[ ! -f "$OTADATA_BIN" ]]; then
    echo "Error: otadata bin not found: ${OTADATA_BIN:-<empty>}" >&2
    echo "Expected ESP-IDF output under: ${ESP_IDF_BUILD_DIR:-<not found>}" >&2
    return 1
  fi

  if [[ "$ERASE_BEFORE_FLASH" -eq 1 ]]; then
    if [[ ! -f "$BOOTLOADER_BIN" || ! -f "$PARTITION_FOR_FLASH" || ! -f "$OTADATA_BIN" ]]; then
      echo "Error: missing bootloader/partition-table/otadata bin required after full erase." >&2
      echo "  bootloader: $BOOTLOADER_BIN" >&2
      echo "  partition : $PARTITION_FOR_FLASH" >&2
      echo "  otadata   : ${OTADATA_BIN:-<empty>}" >&2
      return 1
    fi
    if ! run_espflash_with_connection_profiles write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 0x0 "$BOOTLOADER_BIN"; then
      print_flash_open_port_hints
      return 1
    fi
    if ! run_espflash_with_connection_profiles write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 0x8000 "$PARTITION_FOR_FLASH"; then
      print_flash_open_port_hints
      return 1
    fi
    if ! run_espflash_with_connection_profiles write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 0x19000 "$OTADATA_BIN"; then
      print_flash_open_port_hints
      return 1
    fi
    if ! run_espflash_with_connection_profiles write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 0x20000 "$APP_BIN"; then
      print_flash_open_port_hints
      return 1
    fi
  else
    if ! run_espflash_with_connection_profiles write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 0x19000 "$OTADATA_BIN"; then
      print_flash_open_port_hints
      return 1
    fi
    if ! run_espflash_with_connection_profiles write-bin --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 0x20000 "$APP_BIN"; then
      print_flash_open_port_hints
      return 1
    fi
  fi

  if ! flash_model_partition_if_present; then
    return 1
  fi

  echo ""
  echo -e "${GREEN}✓ Flash complete.${NC}"
  echo ""
  open_monitor_if_requested
  return $?
}

# After build: one prompt for Linux (SSH) or ESP (USB flash), unless --flash or skipped.
prompt_deploy_maybe() {
  [[ -f "$BIN" ]] || return 0
  [[ -t 0 ]] || return 0
  [[ -n "${NO_DEPLOY_PROMPT:-}" ]] && return 0
  [[ "${BEETLE_SKIP_DEPLOY_PROMPT:-}" == "1" ]] && return 0
  [[ -z "${DO_FLASH:-}" ]] || return 0

  local prompt_msg
  if [[ "$BUILD_TARGET" =~ -unknown-linux ]]; then
    prompt_msg="Deploy to Linux device now (SSH)? [y/N]: "
  elif [[ -n "$FLASH_CHIP" ]]; then
    prompt_msg="Flash firmware to ESP32 now (USB serial)? [y/N]: "
  else
    return 0
  fi

  echo ""
  read -r -p "$prompt_msg" deploy_ans
  case "${deploy_ans:-}" in
    [yY]|[yY][eE][sS])
      if [[ "$BUILD_TARGET" =~ -unknown-linux ]]; then
        linux_deploy_main || exit 1
      else
        run_esp_flash_workflow || exit 1
      fi
      exit 0
      ;;
  esac
}

# --- Linux 构建：跳过 ESP 工具链 ---
if [[ "$BUILD_TARGET" =~ -unknown-linux ]]; then
  echo ""
  echo "========== $MSG_LINUX_MODE =========="
  echo "  Target: $BUILD_TARGET"

  if [[ "$BUILD_METHOD" == "remote" ]]; then
    REMOTE_BUILD_ACTIVE=1
    run_linux_remote_build

    case "${REMOTE_BUILD_ROLE:-}" in
      deploy)
        linux_deploy_main
        exit 0
        ;;
      pull_deploy)
        linux_remote_pull_artifact_to_local
        REMOTE_BUILD_ACTIVE=0
        linux_deploy_cleanup_ssh_mux
        REMOTE_TARGET_PREPARED=0
        SSH_MUX_DIR=""
        DEVICE_IP=""
        DEVICE_USER=""
        SSH_PORT=""
        linux_deploy_main
        exit 0
        ;;
      leave)
        echo ""
        echo "========== Remote Build Complete =========="
        echo "  Artifact kept on remote host:"
        echo "    $REMOTE_BUILD_BIN"
        exit 0
        ;;
      *)
        echo "Error: unknown remote build role: ${REMOTE_BUILD_ROLE:-}" >&2
        exit 1
        ;;
    esac
  fi

  # 检查是否在 macOS 上构建 Linux musl
  if [[ "$BUILD_TARGET" =~ -unknown-linux-musl ]] && [[ "$(uname -s)" == "Darwin" ]]; then
    echo "  $MSG_DETECTED_MACOS"

    # 使用 Docker
    if [[ -n "${USE_DOCKER:-}" ]]; then
      run_linux_docker_build "$BUILD_TARGET"

      echo ""
      echo "========== $MSG_BUILD_COMPLETE =========="
      echo "  $MSG_BINARY: $BIN"
      ls -lh "$BIN" 2>/dev/null || echo "  (check target/$BUILD_TARGET/release/beetle)"
      prompt_deploy_maybe
      exit 0
    fi

    # Check and install local musl-cross toolchain when needed.
    if [[ "$BUILD_TARGET" == "x86_64-unknown-linux-musl" ]] && ! command -v x86_64-linux-musl-gcc &>/dev/null; then
      echo ""
      echo "========== Installing musl-cross toolchain =========="
      echo "  x86_64-linux-musl-gcc not found."
      if command -v brew &>/dev/null; then
        echo "  Installing via Homebrew..."
        brew install filosottile/musl-cross/musl-cross
      else
        echo "Error: Neither Docker nor musl-cross found." >&2
        echo "Install one of:" >&2
        echo "  - Docker: https://www.docker.com/products/docker-desktop" >&2
        echo "  - musl-cross: brew install filosottile/musl-cross/musl-cross" >&2
        exit 1
      fi
    fi
    if [[ "$BUILD_TARGET" == "armv7-unknown-linux-musleabihf" ]] && ! command -v arm-linux-musleabihf-gcc &>/dev/null; then
      echo ""
      echo "========== Installing musl-cross toolchain =========="
      echo "  arm-linux-musleabihf-gcc not found."
      if command -v brew &>/dev/null; then
        echo "  Installing via Homebrew..."
        brew install filosottile/musl-cross/musl-cross
      else
        echo "Error: Neither Docker nor musl-cross found." >&2
        echo "Install one of:" >&2
        echo "  - Docker: https://www.docker.com/products/docker-desktop" >&2
        echo "  - musl-cross: brew install filosottile/musl-cross/musl-cross" >&2
        exit 1
      fi
    fi
    if [[ "$BUILD_TARGET" == "aarch64-unknown-linux-musl" ]] && ! command -v aarch64-linux-musl-gcc &>/dev/null; then
      echo ""
      echo "========== Installing musl-cross toolchain =========="
      echo "  aarch64-linux-musl-gcc not found."
      if command -v brew &>/dev/null; then
        echo "  Installing via Homebrew..."
        brew install filosottile/musl-cross/musl-cross
      else
        echo "Error: Neither Docker nor musl-cross found." >&2
        echo "Install one of:" >&2
        echo "  - Docker: https://www.docker.com/products/docker-desktop" >&2
        echo "  - musl-cross: brew install filosottile/musl-cross/musl-cross" >&2
        exit 1
      fi
    fi

    if [[ "$BUILD_TARGET" == "x86_64-unknown-linux-musl" ]] && ! command -v x86_64-linux-musl-gcc &>/dev/null; then
      echo "Error: x86_64-linux-musl-gcc is still not available after installation." >&2
      echo "Hint: restart your shell and verify with: x86_64-linux-musl-gcc --version" >&2
      echo "Or choose Docker build mode to avoid local linker setup." >&2
      exit 1
    fi
    if [[ "$BUILD_TARGET" == "armv7-unknown-linux-musleabihf" ]] && ! command -v arm-linux-musleabihf-gcc &>/dev/null; then
      echo "Error: arm-linux-musleabihf-gcc is still not available after installation." >&2
      echo "Hint: restart your shell and verify with: arm-linux-musleabihf-gcc --version" >&2
      echo "Or choose Docker build mode to avoid local linker setup." >&2
      exit 1
    fi
    if [[ "$BUILD_TARGET" == "aarch64-unknown-linux-musl" ]] && ! command -v aarch64-linux-musl-gcc &>/dev/null; then
      echo "Error: aarch64-linux-musl-gcc is still not available after installation." >&2
      echo "Hint: restart your shell and verify with: aarch64-linux-musl-gcc --version" >&2
      echo "Or choose Docker build mode to avoid local linker setup." >&2
      exit 1
    fi

    # 配置 musl 链接器（x86_64 本地模式）。
    if [[ "$BUILD_TARGET" == "x86_64-unknown-linux-musl" ]]; then
      mkdir -p .cargo
      if ! grep -q "x86_64-unknown-linux-musl" .cargo/config.toml 2>/dev/null; then
        cat >> .cargo/config.toml << 'EOF'

[target.x86_64-unknown-linux-musl]
linker = "x86_64-linux-musl-gcc"
EOF
        echo "  Configured musl linker in .cargo/config.toml"
      fi

      if ! grep -q 'linker = "x86_64-linux-musl-gcc"' .cargo/config.toml 2>/dev/null; then
        cat >> .cargo/config.toml << 'EOF'
linker = "x86_64-linux-musl-gcc"
EOF
        echo "  Added linker for musl target in .cargo/config.toml"
      fi
    fi
    if [[ "$BUILD_TARGET" == "armv7-unknown-linux-musleabihf" ]]; then
      mkdir -p .cargo
      if ! grep -q "armv7-unknown-linux-musleabihf" .cargo/config.toml 2>/dev/null; then
        cat >> .cargo/config.toml << 'EOF'

[target.armv7-unknown-linux-musleabihf]
linker = "arm-linux-musleabihf-gcc"
EOF
        echo "  Configured armv7 musl linker in .cargo/config.toml"
      fi
      if ! grep -q 'linker = "arm-linux-musleabihf-gcc"' .cargo/config.toml 2>/dev/null; then
        cat >> .cargo/config.toml << 'EOF'
linker = "arm-linux-musleabihf-gcc"
EOF
        echo "  Added linker for armv7 musl target in .cargo/config.toml"
      fi
      # Also export linker env to avoid any stale/global Cargo config precedence issues.
      export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER="arm-linux-musleabihf-gcc"
    fi
    if [[ "$BUILD_TARGET" == "aarch64-unknown-linux-musl" ]]; then
      mkdir -p .cargo
      if ! grep -q "aarch64-unknown-linux-musl" .cargo/config.toml 2>/dev/null; then
        cat >> .cargo/config.toml << 'EOF'

[target.aarch64-unknown-linux-musl]
linker = "aarch64-linux-musl-gcc"
EOF
        echo "  Configured aarch64 musl linker in .cargo/config.toml"
      fi
      if ! grep -q 'linker = "aarch64-linux-musl-gcc"' .cargo/config.toml 2>/dev/null; then
        cat >> .cargo/config.toml << 'EOF'
linker = "aarch64-linux-musl-gcc"
EOF
        echo "  Added linker for aarch64 musl target in .cargo/config.toml"
      fi
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="aarch64-linux-musl-gcc"
    fi
  fi

  # 添加 target
  if [[ -z "${USE_DOCKER:-}" ]]; then
    ensure_local_stable_toolchain
    ensure_local_linux_native_build_prereqs
    ensure_local_linux_musl_build_prereqs
  fi
  if ! rustup +stable target list --installed | grep -q "$BUILD_TARGET"; then
    echo "  Adding target: $BUILD_TARGET"
    rustup +stable target add "$BUILD_TARGET"
  fi

  # 跳过 ESP 工具链检查
  SKIP_ESP_TOOLCHAIN=1
else
  # --- ESP toolchain PATH (platform-specific, same role as build.ps1 Set-EspPath) ---
  ensure_local_stable_toolchain
  set_esp_path() {
    for f in "$HOME/export-esp.sh" "$HOME/.espup/export-esp.sh" "$HOME/.local/share/esp-rs/export-esp.sh"; do
      [[ -f "$f" ]] && { source "$f"; return; }
    done
    for d in "$HOME/.rustup/toolchains/esp/"*/bin "$HOME/.rustup/toolchains/esp/xtensa-esp-elf/"*/xtensa-esp-elf/bin; do
      if [[ -x "${d}/xtensa-esp32s3-elf-gcc" || -x "${d}/riscv32-esp-elf-gcc" ]]; then
        export PATH="$d:$PATH"
        return
      fi
    done
  }
  set_esp_path

  # --- Install ESP toolchain if missing (same as build.ps1) ---
  if ! command -v xtensa-esp32s3-elf-gcc &>/dev/null && ! command -v riscv32-esp-elf-gcc &>/dev/null; then
    echo ""
    echo "========== Step: Installing ESP Rust toolchain (espup) =========="
    echo "  ESP GCC toolchains not found. Running espup install."
    if ! command -v espup &>/dev/null; then
      echo ">>> Installing espup (using stable)..."
      RUSTUP_TOOLCHAIN=stable cargo install espup
      export PATH="${HOME}/.cargo/bin:${PATH}"
    fi
    espup install
    set_esp_path
    command -v xtensa-esp32s3-elf-gcc &>/dev/null || command -v riscv32-esp-elf-gcc &>/dev/null || {
      echo "Error: ESP GCC toolchains still not found after espup install" >&2
      exit 1
    }
  fi

  # --- Install ldproxy if missing (same as build.ps1; no Windows prebuilt on Mac/Linux) ---
  if ! command -v ldproxy &>/dev/null; then
    echo ""
    echo "========== Step: Installing ldproxy (linker wrapper) =========="
    echo ">>> Installing ldproxy (using stable)..."
    RUSTUP_TOOLCHAIN=stable cargo install ldproxy
    export PATH="${HOME}/.cargo/bin:${PATH}"
  fi
fi

# --- Select sdkconfig defaults chain for ESP builds ---
if [[ -z "${SKIP_ESP_TOOLCHAIN:-}" ]]; then
  SDKCONFIG_DEFAULTS_CHAIN=("sdkconfig.defaults")
  if [[ -n "${TARGET_MCU:-}" ]]; then
    SDKCONFIG_DEFAULTS_CHAIN+=("sdkconfig.defaults.${TARGET_MCU}")
  fi
  if [[ -n "${BOARD_SDKCONFIG_OVERLAY:-}" ]]; then
    SDKCONFIG_DEFAULTS_CHAIN+=("${BOARD_SDKCONFIG_OVERLAY}")
  fi
  ESP_IDF_SDKCONFIG_DEFAULTS="$(IFS=';'; printf '%s' "${SDKCONFIG_DEFAULTS_CHAIN[*]}")"
  export ESP_IDF_SDKCONFIG_DEFAULTS
fi

# --- Build args: inject default ESP --target when missing (same as build.ps1); no longer rely on .cargo [build] target ---
RELEASE_ARGS=()
HAS_TARGET=0
for a in "${BUILD_ARGS[@]}"; do [[ "$a" == "--target" ]] && HAS_TARGET=1; done
[[ $HAS_TARGET -eq 0 ]] && RELEASE_ARGS+=(--target "$BUILD_TARGET")
[[ -n "$BUILD_FEATURES" ]] && RELEASE_ARGS+=($BUILD_FEATURES)
RELEASE_ARGS+=("${BUILD_ARGS[@]}")

# --- Build (same as build.ps1) ---
echo ""
echo "========== Step: Building release =========="
echo "  Target: $BUILD_TARGET  |  Root: $SCRIPT_ROOT"

if [[ -z "${SKIP_ESP_TOOLCHAIN:-}" ]]; then
  refresh_esp_component_graph_cache || exit 1
fi

# Linux 构建用 stable 工具链
if [[ "$BUILD_TARGET" =~ -unknown-linux ]]; then
  if ! cargo +stable build --release "${RELEASE_ARGS[@]}"; then
    # Auto fallback: local musl build failed on macOS, retry with Docker if available.
    if [[ "$(uname -s)" == "Darwin" ]] && [[ "$BUILD_TARGET" =~ -unknown-linux-musl ]] && [[ -z "${USE_DOCKER:-}" ]] && command -v docker &>/dev/null && docker info &>/dev/null; then
      echo ""
      echo "Local toolchain build failed. Auto-fallback to Docker build..."
      run_linux_docker_build "$BUILD_TARGET"
      echo ""
      echo "========== $MSG_BUILD_COMPLETE =========="
      echo "  $MSG_BINARY: $BIN"
      ls -lh "$BIN" 2>/dev/null || echo "  (check target/$BUILD_TARGET/release/beetle)"
      prompt_deploy_maybe
      exit 0
    fi

    echo "" >&2
    echo "Build failed for target: $BUILD_TARGET" >&2
    if [[ "$BUILD_TARGET" == "x86_64-unknown-linux-musl" ]] && [[ "$(uname -s)" == "Darwin" ]]; then
      echo "Common fixes on macOS:" >&2
      echo "  1) Ensure target is installed on stable toolchain:" >&2
      echo "     rustup +stable target add x86_64-unknown-linux-musl" >&2
      echo "  2) Ensure musl linker is available:" >&2
      echo "     x86_64-linux-musl-gcc --version" >&2
      echo "  3) Ensure .cargo/config.toml contains:" >&2
      echo "     [target.x86_64-unknown-linux-musl]" >&2
      echo "     linker = \"x86_64-linux-musl-gcc\"" >&2
      echo "  4) If your default toolchain is esp, always use +stable for rustup target commands." >&2
      echo "  5) Prefer Docker mode if linker errors persist (recommended)." >&2
    elif [[ "$BUILD_TARGET" == "armv7-unknown-linux-musleabihf" ]] && [[ "$(uname -s)" == "Darwin" ]]; then
      echo "Common fixes on macOS (armv7):" >&2
      echo "  1) Prefer Docker mode for armv7 (recommended)." >&2
      echo "  2) Ensure target is installed on stable toolchain:" >&2
      echo "     rustup +stable target add armv7-unknown-linux-musleabihf" >&2
    elif [[ "$BUILD_TARGET" == "aarch64-unknown-linux-musl" ]] && [[ "$(uname -s)" == "Darwin" ]]; then
      echo "Common fixes on macOS (aarch64):" >&2
      echo "  1) Prefer Docker mode for aarch64 (recommended)." >&2
      echo "  2) Ensure target is installed on stable toolchain:" >&2
      echo "     rustup +stable target add aarch64-unknown-linux-musl" >&2
      echo "  3) Ensure musl linker is available:" >&2
      echo "     aarch64-linux-musl-gcc --version" >&2
    fi
    exit 1
  fi
else
  if [[ "$BUILD_PROFILE" == "release" ]]; then
    cargo build --release "${RELEASE_ARGS[@]}"
  else
    cargo build --profile "$BUILD_PROFILE" "${RELEASE_ARGS[@]}"
  fi
fi

if [[ ! "$BUILD_TARGET" =~ -unknown-linux ]]; then
  generate_app_bin_from_elf || exit 1
fi

# --- After build: deploy prompt or --flash (ESP only) ---
echo ""
echo "========== $MSG_BUILD_COMPLETE =========="
echo "  $MSG_BINARY: $BIN"
ls -lh "$BIN" 2>/dev/null || true
if [[ -f "$APP_BIN" ]]; then
  echo "  Firmware app bin: $APP_BIN"
  ls -lh "$APP_BIN" 2>/dev/null || true
fi

if [[ -n "$DO_FLASH" ]]; then
  if [[ "$BUILD_TARGET" =~ -unknown-linux ]]; then
    echo -e "${YELLOW}Note: --flash / --flash-update apply to ESP builds only (current target is Linux).${NC}" >&2
  else
    run_esp_flash_workflow || exit 1
    exit 0
  fi
fi

prompt_deploy_maybe

echo ""
if [[ "$BUILD_TARGET" =~ -unknown-linux ]]; then
  echo "  Deploy later: ./build.sh --deploy-linux"
else
  if [[ -n "$FLASH_CHIP" ]]; then
    echo "  Flash later: run ./build.sh again and answer Yes at the deploy prompt, or: ./build.sh --flash / --flash-update"
  fi
fi
exit 0

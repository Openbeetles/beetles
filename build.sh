#!/usr/bin/env bash
# One-shot build script with interactive platform selection.
set -e
SCRIPT_ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_ROOT"

# Colors (build + Linux SSH deploy)
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

show_help() {
  cat <<'EOF'
Usage:
  ./build.sh [--flash | --flash-update] [--no-monitor] [--no-deploy] [--deploy-linux] [cargo build args...]

Linux SSH deploy only (no compile; needs an existing target/*/release/beetle):
  ./build.sh --deploy-linux

Default (interactive TTY): after a successful build, asks whether to deploy:
  - Linux targets → SSH upload (same flow as --deploy-linux)
  - ESP targets   → USB flash (port + erase menu unless --flash-update)

Non-interactive / CI: use --no-deploy, BEETLE_SKIP_DEPLOY_PROMPT=1, or redirect stdin.

Skip the question and flash ESP immediately (automation):
  --flash          Build then flash ESP; interactive erase menu (default: update only, keep NVS).
  --flash-update   Build then flash ESP without erase (no erase menu).

Quick examples:
  ./build.sh
  TARGET=linux ./build.sh
  TARGET=linux-armv7 ./build.sh
  TARGET=esp ./build.sh
  TARGET=esp ./build.sh --flash
  ./build.sh --deploy-linux

Notes:
  - On macOS building Linux musl, auto mode uses Docker only if the daemon is running; otherwise musl-cross (Homebrew).
  - Force local: BUILD_METHOD=local ./build.sh
  - Force Docker: BUILD_METHOD=docker ./build.sh
  - For Linux builds, this script uses Rust stable toolchain.
EOF
}

# Fast-path help.
for arg in "$@"; do
  case "$arg" in
    -h|--help) show_help; exit 0 ;;
  esac
done

MSG_TITLE="Beetle Build Script"
MSG_SELECT_PLATFORM="Select build platform:"
MSG_PLATFORM_ESP="ESP32-S3 Firmware (default)"
MSG_PLATFORM_LINUX="Linux x86_64"
MSG_PLATFORM_LINUX_ARMV7="Linux armv7 (32-bit ARM hard-float)"
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
export PATH="${HOME}/.cargo/bin:${PATH}"

# --- Parse args (same as build.ps1) ---
DO_FLASH=""
DO_DEPLOY_LINUX=""
NO_MONITOR=""
NO_DEPLOY_PROMPT=""
FLASH_NO_ERASE=""
BUILD_METHOD="${BUILD_METHOD:-auto}" # auto | docker | local
BUILD_ARGS=()
for arg in "$@"; do
  case "$arg" in
    -h|--help)       show_help; exit 0 ;;
    --flash)         DO_FLASH=1 ;;
    --flash-update)  DO_FLASH=1; FLASH_NO_ERASE=1 ;;
    --no-monitor)    NO_MONITOR=1 ;;
    --no-deploy)     NO_DEPLOY_PROMPT=1 ;;
    --deploy-linux)  DO_DEPLOY_LINUX=1 ;;
    *)               BUILD_ARGS+=("$arg") ;;
  esac
done

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

    if [ -f "target/aarch64-unknown-linux-musl/release/beetle" ]; then
        binaries+=("aarch64")
    fi

    echo "${binaries[@]}"
}

# Select architecture
linux_deploy_select_arch() {
    local available=($(linux_deploy_detect_binaries))

    if [ ${#available[@]} -eq 0 ]; then
        echo -e "${RED}Error: No compiled binaries found${NC}"
        echo "Please run ./build.sh first"
        exit 1
    fi

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
    else
        read -p "Select architecture [1-${#available[@]}]: " choice
        choice=${choice:-1}
        SELECTED_ARCH="${available[$((choice-1))]}"
    fi

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
        aarch64)
            BINARY_PATH="target/aarch64-unknown-linux-musl/release/beetle"
            EMBED_DEPS_ARCH="aarch64"
            ;;
        *)
            echo -e "${RED}Error: unknown arch key: $SELECTED_ARCH${NC}"
            exit 1
            ;;
    esac

    echo ""
}

# Persist last successful target (IP / user / port). Password is never stored.
# Path: ~/.config/beetle/deploy-linux.defaults (mode 600).
DEPLOY_DEFAULTS_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/beetle"
DEPLOY_DEFAULTS_FILE="$DEPLOY_DEFAULTS_DIR/deploy-linux.defaults"

linux_deploy_load_deploy_defaults() {
    DEFAULT_DEVICE_IP=""
    DEFAULT_DEVICE_USER="root"
    DEFAULT_SSH_PORT="22"
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
        esac
    done <"$DEPLOY_DEFAULTS_FILE"
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
        } >"${DEPLOY_DEFAULTS_FILE}.tmp"
        mv "${DEPLOY_DEFAULTS_FILE}.tmp" "$DEPLOY_DEFAULTS_FILE"
    )
}

# Input device information
linux_deploy_input_device_info() {
    echo "========== Device Information =========="
    echo ""
    linux_deploy_load_deploy_defaults

    if [ -n "$DEFAULT_DEVICE_IP" ]; then
        echo -e "${GREEN}Saved target: ${DEFAULT_DEVICE_USER}@${DEFAULT_DEVICE_IP}:${DEFAULT_SSH_PORT}${NC}"
        echo "(Press Enter to keep; password is not saved — use SSH keys for passwordless login)"
        echo ""
    fi

    if [ -n "$DEFAULT_DEVICE_IP" ]; then
        read -p "Device IP address [$DEFAULT_DEVICE_IP]: " DEVICE_IP
    else
        read -p "Device IP address: " DEVICE_IP
    fi
    DEVICE_IP=${DEVICE_IP:-$DEFAULT_DEVICE_IP}
    if [ -z "$DEVICE_IP" ]; then
        echo -e "${RED}Error: IP address cannot be empty${NC}"
        exit 1
    fi

    read -p "Username [${DEFAULT_DEVICE_USER}]: " DEVICE_USER
    DEVICE_USER=${DEVICE_USER:-$DEFAULT_DEVICE_USER}

    read -p "SSH port [${DEFAULT_SSH_PORT}]: " SSH_PORT
    SSH_PORT=${SSH_PORT:-$DEFAULT_SSH_PORT}

    if ! [[ "$SSH_PORT" =~ ^[0-9]+$ ]] || [ "$SSH_PORT" -lt 1 ] || [ "$SSH_PORT" -gt 65535 ]; then
        echo -e "${RED}Error: SSH port must be a number between 1 and 65535${NC}"
        exit 1
    fi

    echo ""
    echo -e "${BLUE}Target device: ${DEVICE_USER}@${DEVICE_IP}:${SSH_PORT}${NC}"
    echo ""
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

    # One remote shell (mapfile needs bash 4+; keep portable for macOS /bin/bash)
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

    # Architecture match check
    case "$DEVICE_ARCH" in
        x86_64)
            if [ "$SELECTED_ARCH" != "x86_64" ] && [ "$SELECTED_ARCH" != "x86_64-gnu" ]; then
                echo -e "${YELLOW}⚠ Warning: Device is x86_64, but selected $SELECTED_ARCH${NC}"
            fi
            ;;
        armv7l)
            if [ "$SELECTED_ARCH" != "armv7" ]; then
                echo -e "${YELLOW}⚠ Warning: Device is armv7l, but selected $SELECTED_ARCH${NC}"
            fi
            ;;
        aarch64)
            if [ "$SELECTED_ARCH" != "aarch64" ]; then
                echo -e "${YELLOW}⚠ Warning: Device is aarch64, but selected $SELECTED_ARCH${NC}"
            fi
            ;;
    esac

    echo ""
}

# Select deployment mode
linux_deploy_select_deploy_mode() {
    echo "========== Deployment Mode =========="
    echo ""
    echo "  1) Quick deploy (binary only)"
    echo "  2) Full deploy (binary + systemd service)"
    echo "  3) Update binary only"
    echo ""

    read -p "Select mode [1-3] (default 2): " mode
    DEPLOY_MODE=${mode:-2}
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
    ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        "mkdir -p /opt/beetle/bin"
    for f in "$embed"/*; do
        [ -f "$f" ] || continue
        case "$(basename "$f")" in
            README*|*.md|*.txt) continue ;;
        esac
        scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" "$f" \
            "${DEVICE_USER}@${DEVICE_IP}:/opt/beetle/bin/"
    done
    ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        'for f in /opt/beetle/bin/*; do [ -f "$f" ] && chmod a+x "$f"; done'
    EMBED_DEPS_UPLOADED=1
    echo -e "${GREEN}✓ Bundled tools uploaded (beetle prefers /opt/beetle/bin)${NC}"
}

# Upload files
linux_deploy_upload_files() {
    echo "========== Uploading Files =========="
    echo ""

    echo "Creating directories..."
    ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" \
        "mkdir -p /opt/beetle /opt/beetle/bin /var/lib/beetle"

    linux_deploy_upload_embed_deps

    echo "Uploading binary..."
    scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" "$BINARY_PATH" \
        "${DEVICE_USER}@${DEVICE_IP}:/opt/beetle/beetle"

    if [ "$DEPLOY_MODE" = "2" ]; then
        echo "Uploading systemd service..."
        scp "${SSH_MUX_OPTS[@]}" -P "$SSH_PORT" packaging/linux/beetle.service \
            "${DEVICE_USER}@${DEVICE_IP}:/tmp/"
    fi

    echo -e "${GREEN}✓ Upload complete${NC}"
    echo ""
    linux_deploy_report_wifi_tools_on_device
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

# Install service
linux_deploy_install_service() {
    echo "========== Installing Service =========="
    echo ""

    ssh "${SSH_MUX_OPTS[@]}" -p "$SSH_PORT" "${DEVICE_USER}@${DEVICE_IP}" << 'REMOTE_EOF'
chmod +x /opt/beetle/beetle

if [ -f /tmp/beetle.service ]; then
    cp /tmp/beetle.service /etc/systemd/system/
    systemctl daemon-reload
    echo "✓ systemd service installed"
fi
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
    echo "  1. Start beetle:"
    echo "     - With systemd: systemctl start beetle"
    echo "     - Without systemd: ssh -p $SSH_PORT ${DEVICE_USER}@${DEVICE_IP}"
    echo "       then: nohup /opt/beetle/beetle >> /var/log/beetle.log 2>&1 &"
    echo ""
    echo "  2. Configure WiFi (after WiFi stack works):"
    echo "     Hotspot SSID Beetle → http://DEVICE_IP/ (or http://192.168.4.1 on SoftAP)"
    echo ""
    echo "Configuration files:"
    echo "  - State directory: /var/lib/beetle"
    echo "  - Service config: /etc/systemd/system/beetle.service"
    echo ""
}

# 主流程
linux_deploy_main() {
    echo ""
    echo "=========================================="
    echo "  Beetle Linux Deployment"
    echo "=========================================="
    echo ""
    linux_deploy_select_arch
    linux_deploy_fetch_embed_deps_from_url
    linux_deploy_input_device_info
    linux_deploy_setup_ssh_mux
    trap linux_deploy_cleanup_ssh_mux EXIT INT TERM
    linux_deploy_test_connection
    linux_deploy_detect_device_arch
    linux_deploy_select_deploy_mode
    linux_deploy_prompt_wifi_helpers_or_continue
    linux_deploy_upload_files

    if [ "$DEPLOY_MODE" = "2" ]; then
        linux_deploy_install_service
    fi

    linux_deploy_show_next_steps
}


if [[ -n "$DO_DEPLOY_LINUX" ]]; then
  linux_deploy_main
  exit $?
fi

command -v cargo &>/dev/null || { echo "Error: cargo not found. Install Rust: https://rustup.rs" >&2; exit 1; }

run_linux_docker_build() {
  local target="$1"
  echo "  $MSG_USING_DOCKER"
  echo ""
  echo "========== $MSG_BUILD_IN_DOCKER =========="
  if [[ "$target" == "x86_64-unknown-linux-musl" ]]; then
    docker run --rm -v "$SCRIPT_ROOT":/workspace -w /workspace \
      rust:latest \
      bash -c "rustup target add x86_64-unknown-linux-musl && cargo build --release --target x86_64-unknown-linux-musl"
  elif [[ "$target" == "armv7-unknown-linux-musleabihf" ]]; then
    docker run --rm -v "$SCRIPT_ROOT":/home/rust/src -w /home/rust/src \
      messense/rust-musl-cross:armv7-musleabihf \
      cargo build --release --target armv7-unknown-linux-musleabihf
  else
    echo "Error: Docker build not supported for target: $target" >&2
    exit 1
  fi
}
select_build_platform() {
  # If TARGET env is set, skip interactive prompt.
  if [[ -n "${TARGET:-}" ]]; then
    case "${TARGET}" in
      esp|esp32) PLATFORM_CHOICE=1; return 0 ;;  # ESP32
      linux) PLATFORM_CHOICE=2; return 0 ;;      # Linux
      linux-armv7|armv7) PLATFORM_CHOICE=3; return 0 ;;
      *) echo "Error: Unknown TARGET=$TARGET. Use 'esp', 'linux', or 'linux-armv7'" >&2; exit 1 ;;
    esac
  fi

  # If --flash / --flash-update is set, default to ESP32.
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
  echo "  1) $MSG_PLATFORM_ESP"
  echo "  2) $MSG_PLATFORM_LINUX"
  echo "  3) $MSG_PLATFORM_LINUX_ARMV7"
  echo ""

  while true; do
    read -r -p "$MSG_INPUT_OPTION [1-3] ($MSG_PRESS_ENTER 1): " choice
    choice=${choice:-1}
    case "$choice" in
      1) PLATFORM_CHOICE=1; return 0 ;;
      2) PLATFORM_CHOICE=2; return 0 ;;
      3) PLATFORM_CHOICE=3; return 0 ;;
      *) echo "$MSG_INVALID_OPTION 1, 2, or 3" ;;
    esac
  done
}

# Execute platform selection.
PLATFORM_CHOICE=1
select_build_platform

if [[ $PLATFORM_CHOICE -eq 2 || $PLATFORM_CHOICE -eq 3 ]]; then
  # Linux 构建
  echo ""
  echo "========== $MSG_LINUX_MODE =========="
  if [[ $PLATFORM_CHOICE -eq 3 ]]; then
    BUILD_TARGET="armv7-unknown-linux-musleabihf"
  fi

  # 检测当前系统
  CURRENT_OS="$(uname -s)"
  if [[ "$CURRENT_OS" == "Linux" ]]; then
    # 在 Linux 上，直接用原生构建
    if [[ $PLATFORM_CHOICE -eq 2 ]]; then
      BUILD_TARGET="x86_64-unknown-linux-gnu"
    fi
    echo "  $MSG_DETECTED_LINUX"
  elif [[ "$CURRENT_OS" == "Darwin" ]]; then
    # On macOS, cross-compile to Linux.
    echo "  $MSG_DETECTED_MACOS"
    if [[ $PLATFORM_CHOICE -eq 2 ]]; then
      BUILD_TARGET="x86_64-unknown-linux-musl"
      LOCAL_LINKER_CMD="x86_64-linux-musl-gcc"
    else
      BUILD_TARGET="armv7-unknown-linux-musleabihf"
      LOCAL_LINKER_CMD="arm-linux-musleabihf-gcc"
    fi

    # Docker CLI alone is not enough (Desktop may be off); require a running daemon for auto mode.
    HAS_DOCKER_CLI=0
    command -v docker &>/dev/null && HAS_DOCKER_CLI=1
    HAS_DOCKER_DAEMON=0
    if [[ $HAS_DOCKER_CLI -eq 1 ]] && docker info &>/dev/null; then
      HAS_DOCKER_DAEMON=1
    fi
    HAS_LOCAL_LINKER=0
    command -v "$LOCAL_LINKER_CMD" &>/dev/null && HAS_LOCAL_LINKER=1

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
        echo "Error: BUILD_METHOD must be one of: auto, docker, local" >&2
        exit 1
        ;;
    esac
  else
    echo "$MSG_UNKNOWN_OS $CURRENT_OS, $MSG_TRY_NATIVE"
    BUILD_TARGET="x86_64-unknown-linux-gnu"
  fi

  BUILD_FEATURES=""
  SKIP_ESP_TOOLCHAIN=1
else
  # ESP32 构建
  echo ""
  echo "========== $MSG_ESP_MODE =========="

  # --- BOARD => target/features from board_presets.toml ---
  BUILD_TARGET="xtensa-esp32s3-espidf"
  BUILD_FEATURES=""
fi
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
  if [[ -z "$PARTITION_TABLE" ]]; then
    case "$BOARD" in
      esp32-s3-8mb)  PARTITION_TABLE=partitions_8mb.csv ;;
      esp32-s3-32mb) PARTITION_TABLE=partitions_32mb.csv ;;
      *)             PARTITION_TABLE=partitions.csv ;;
    esac
  fi
else
  PARTITION_TABLE=partitions.csv
fi
# Command-line --target overrides BOARD (same as build.ps1)
for (( i=0; i < ${#BUILD_ARGS[@]}; i++ )); do
  if [[ "${BUILD_ARGS[$i]}" == "--target" ]] && (( i+1 < ${#BUILD_ARGS[@]} )); then
    BUILD_TARGET="${BUILD_ARGS[$i+1]}"
    break
  fi
done
# Sanitize target (no path chars)
if [[ ! "$BUILD_TARGET" =~ ^[a-zA-Z0-9_-]+$ ]]; then
  echo "Error: Invalid --target (no path chars): $BUILD_TARGET" >&2
  exit 1
fi

# Derive chip from target for flash (same as build.ps1)
FLASH_CHIP=""
if [[ "$BUILD_TARGET" =~ (esp32[a-z0-9]+) ]]; then
  FLASH_CHIP="${BASH_REMATCH[1]}"
fi

# Print detected hardware / build config (same as build.ps1 Write-BuildStatus)
echo ""
echo "========== Detected hardware / build config =========="
echo "  Project root:      $SCRIPT_ROOT"
echo "  Build target:      $BUILD_TARGET"
echo "  BOARD (optional):  ${BOARD:-(not set)}"
echo "  Partition table:   $PARTITION_TABLE"
echo "  Chip (for flash):  ${FLASH_CHIP:-(N/A)}"
echo "  Features:          ${BUILD_FEATURES:-(none)}"
echo ""

# --- clean: cargo clean then exit (no short-path on Mac/Linux) ---
if printf '%s\n' "${BUILD_ARGS[@]}" | grep -qx "clean"; then
  echo "========== Step: Cleaning build artifacts =========="
  echo "  Running: cargo clean (project root)..."
  CLEAN_ARGS=()
  for a in "${BUILD_ARGS[@]}"; do [[ "$a" != "clean" ]] && CLEAN_ARGS+=("$a"); done
  cargo clean "${CLEAN_ARGS[@]}"
  exit $?
fi

# effectiveTargetDir (same as build.ps1)
EFFECTIVE_TARGET_DIR="${CARGO_TARGET_DIR:-$SCRIPT_ROOT/target}"
RELEASE_DIR="$EFFECTIVE_TARGET_DIR/$BUILD_TARGET/release"
BIN="$RELEASE_DIR/beetle"
BOOTLOADER_BIN="$RELEASE_DIR/bootloader.bin"
PARTITION_TABLE_BIN="$RELEASE_DIR/partition-table.bin"
PARTITION_CSV="$SCRIPT_ROOT/$PARTITION_TABLE"
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
  echo -e "${RED}Flash / serial open failed.${NC}" >&2
  echo "  Common fixes:" >&2
  echo "    1) Hold BOOT, tap RESET, run ./build.sh again (deploy Yes or --flash) within a few seconds (ROM download mode)." >&2
  echo "    2) Quit anything using the port: Serial Monitor, screen/minicom, another IDE, \`idf.py monitor\`." >&2
  echo "    3) macOS: try direct USB (no hub); unplug/replug; or \`ESPFLASH_PORT=/dev/cu.… ./build.sh --flash\`." >&2
  echo "    4) If you use conda base, try: \`conda deactivate\` then flash (rare toolchain PATH issues)." >&2
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
# Interactive port selection when ESPFLASH_PORT not set (same as build.ps1 Get-FlashPort)
# Only the chosen port is printed to stdout; messages go to stderr.
get_flash_port() {
  if [[ -n "${ESPFLASH_PORT:-}" ]]; then
    valid_flash_port "$ESPFLASH_PORT" || { echo "Error: ESPFLASH_PORT must be a valid device path (e.g. /dev/ttyUSB0). Got: $ESPFLASH_PORT" >&2; exit 1; }
    echo "$ESPFLASH_PORT"
    return
  fi
  PORTS=()
  if [[ "$(uname -s)" = "Linux" ]]; then
    for f in /dev/ttyUSB* /dev/ttyACM*; do [[ -e "$f" ]] && PORTS+=("$f"); done
  else
    for f in /dev/cu.usbmodem* /dev/cu.usbserial* /dev/cu.SLAB* /dev/cu.wchusbserial* /dev/cu.UART*; do [[ -e "$f" ]] && PORTS+=("$f"); done
  fi
  if [[ ${#PORTS[@]} -eq 0 ]]; then
    echo "No serial ports found. Plug in the board or set ESPFLASH_PORT=/dev/..." >&2
    exit 1
  fi
  if [[ ${#PORTS[@]} -eq 1 ]]; then
    echo "  Detected 1 serial port: ${PORTS[0]}" >&2
    echo "${PORTS[0]}"
    return
  fi
  echo "  Detected ${#PORTS[@]} serial ports. Select port to flash (ESP board):" >&2
  for i in "${!PORTS[@]}"; do echo "  $((i+1)). ${PORTS[i]}" >&2; done
  # macOS: one board often appears as both cu.usbmodem* and cu.wchusbserial* (same USB serial in the name).
  # Opening the WCH alias sometimes fails with "Failed to open serial port" while usbmodem works (or vice versa).
  if [[ "$(uname -s)" != "Linux" ]]; then
    echo -e "${YELLOW}  Tip: Prefer a cu.usbmodem* entry for ESP32-S3 native USB; if you see wchusbserial with the same ID as usbmodem, avoid the duplicate — pick the other.${NC}" >&2
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
  CHOSEN_PORT=$(get_flash_port)
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
  echo "  Features:          ${BUILD_FEATURES:-(none)}"
  echo -e "  ${BLUE}Serial port:${NC}       $CHOSEN_PORT"
  echo "  Partition table:   $PARTITION_FOR_FLASH"
  echo "  Bootloader:        $BOOTLOADER_BIN"
  echo "  Firmware binary:   $BIN"
  echo ""

  echo "========== Checking connection =========="
  echo ""
  echo "  Serial port occupancy (lsof):" >&2
  warn_serial_port_busy "$CHOSEN_PORT"
  echo ""
  if espflash board-info --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" 2>/dev/null; then
    echo -e "${GREEN}✓ board-info OK${NC}"
  else
    echo -e "${YELLOW}⚠ Could not read board-info from $CHOSEN_PORT (connection or chip mismatch).${NC}"
    echo "  If flash then fails to open the port, use download mode: hold BOOT, tap RESET, flash within a few seconds."
    echo "  Also close any Serial Monitor / screen / idf.py monitor using this port."
  fi
  echo ""

  select_flash_mode "$CHOSEN_PORT" "$BUILD_TARGET"

  if [[ "$ERASE_BEFORE_FLASH" -eq 1 ]]; then
    echo "========== Erasing entire flash =========="
    echo ""
    echo "  Port: $CHOSEN_PORT  |  Chip: $FLASH_CHIP"
    if ! espflash erase-flash --port "$CHOSEN_PORT" --chip "$FLASH_CHIP"; then
      echo "" >&2
      echo -e "${RED}Erase failed.${NC} Common causes:" >&2
      echo "  - Port in use or disconnected: unplug and replug; set ESPFLASH_PORT=/dev/cu.xxx if multiple ports" >&2
      echo "  - Board not in download mode: hold BOOT, tap RESET, run this command again within a few seconds" >&2
      echo "  - Wrong chip: build target is $BUILD_TARGET (chip $FLASH_CHIP); use the matching board" >&2
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
  echo "  Binary: $BIN"
  echo "  Partition table: $PARTITION_FOR_FLASH"
  local FLASH_OK=0
  if [[ -n "$NO_MONITOR" ]]; then
    if espflash flash --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" "${FLASH_EXTRA[@]}" "$BIN"; then
      FLASH_OK=1
    fi
  else
    if espflash flash --port "$CHOSEN_PORT" --chip "$FLASH_CHIP" "${FLASH_EXTRA[@]}" --monitor "$BIN"; then
      FLASH_OK=1
    fi
  fi
  if [[ "$FLASH_OK" -eq 1 ]]; then
    echo ""
    echo -e "${GREEN}✓ Flash complete.${NC}"
    echo ""
    return 0
  fi
  print_flash_open_port_hints
  return 1
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
  fi

  # 添加 target
  if ! rustup +stable target list --installed | grep -q "$BUILD_TARGET"; then
    echo "  Adding target: $BUILD_TARGET"
    rustup +stable target add "$BUILD_TARGET"
  fi

  # 跳过 ESP 工具链检查
  SKIP_ESP_TOOLCHAIN=1
else
  # --- ESP toolchain PATH (platform-specific, same role as build.ps1 Set-EspPath) ---
  set_esp_path() {
    for f in "$HOME/export-esp.sh" "$HOME/.espup/export-esp.sh" "$HOME/.local/share/esp-rs/export-esp.sh"; do
      [[ -f "$f" ]] && { source "$f"; return; }
    done
    for d in "$HOME/.rustup/toolchains/esp/xtensa-esp-elf/"*/xtensa-esp-elf/bin; do
      [[ -x "${d}/xtensa-esp32s3-elf-gcc" ]] 2>/dev/null && { export PATH="$d:$PATH"; return; }
    done
  }
  set_esp_path

  # --- Install ESP toolchain if missing (same as build.ps1) ---
  if ! command -v xtensa-esp32s3-elf-gcc &>/dev/null && ! command -v riscv32-esp-elf-gcc &>/dev/null; then
    echo ""
    echo "========== Step: Installing ESP Rust toolchain (espup) =========="
    echo "  xtensa-esp32s3-elf-gcc not found. Running espup install."
    if ! command -v espup &>/dev/null; then
      echo ">>> Installing espup (using stable)..."
      RUSTUP_TOOLCHAIN=stable cargo install espup
      export PATH="${HOME}/.cargo/bin:${PATH}"
    fi
    espup install
    set_esp_path
    command -v xtensa-esp32s3-elf-gcc &>/dev/null || command -v riscv32-esp-elf-gcc &>/dev/null || {
      echo "Error: xtensa-esp32s3-elf-gcc still not found after espup install" >&2
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

# --- Write sdkconfig board overlay (仅 ESP 目标) ---
if [[ -z "${SKIP_ESP_TOOLCHAIN:-}" ]]; then
  BOARD_SDKCONFIG="$SCRIPT_ROOT/sdkconfig.defaults.esp32s3.board"
  # 与 sdkconfig.defaults.esp32s3 一致：CMake 工程根为 esp-idf-sys 的 out/，分区表路径需相对 out/ 指向仓库根 CSV。
  case "$PARTITION_TABLE" in
    partitions_8mb.csv)
      printf '%s\n' 'CONFIG_ESPTOOLPY_FLASHSIZE_8MB=y' '# CONFIG_ESPTOOLPY_FLASHSIZE_16MB is not set' 'CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="../../../../../../partitions_8mb.csv"' > "$BOARD_SDKCONFIG"
      ;;
    partitions_32mb.csv)
      printf '%s\n' 'CONFIG_ESPTOOLPY_FLASHSIZE_32MB=y' '# CONFIG_ESPTOOLPY_FLASHSIZE_16MB is not set' 'CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="../../../../../../partitions_32mb.csv"' > "$BOARD_SDKCONFIG"
      ;;
    *)
      printf '%s\n' 'CONFIG_ESPTOOLPY_FLASHSIZE_16MB=y' 'CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="../../../../../../partitions.csv"' > "$BOARD_SDKCONFIG"
      ;;
  esac
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
    fi
    exit 1
  fi
else
  cargo build --release "${RELEASE_ARGS[@]}"
fi

# --- After build: deploy prompt or --flash (ESP only) ---
echo ""
echo "========== $MSG_BUILD_COMPLETE =========="
echo "  $MSG_BINARY: $BIN"
ls -lh "$BIN" 2>/dev/null || true

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

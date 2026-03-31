#!/usr/bin/env bash
set -euo pipefail

# voicerouter installation script
# Downloads and installs voicerouter binary for your architecture
# Usage: curl -fsSL https://raw.githubusercontent.com/yggdjc/ygg-voicerouter/main/scripts/install.sh | bash

# Configuration
REPO="yggdjc/ygg-voicerouter"
GITHUB_BASE="https://github.com/${REPO}"
RELEASES_URL="${GITHUB_BASE}/releases"
INSTALL_DIR="${HOME}/.local/bin"
MODEL_DIR="${HOME}/.cache/voicerouter/models"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Helper functions
log_info() {
    echo -e "${GREEN}▸${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}⚠${NC} $1"
}

log_error() {
    echo -e "${RED}✗${NC} $1" >&2
}

log_success() {
    echo -e "${GREEN}✓${NC} $1"
}

# Detect architecture
detect_arch() {
    local arch
    arch=$(uname -m)

    case "$arch" in
        x86_64)
            echo "x86_64"
            ;;
        aarch64|arm64)
            echo "aarch64"
            ;;
        *)
            log_error "Unsupported architecture: $arch"
            log_info "Supported: x86_64, aarch64"
            exit 1
            ;;
    esac
}

# Get latest release version
get_latest_version() {
    local version
    # Try to get the latest release tag
    version=$(curl -fsSL "${RELEASES_URL}/latest" 2>/dev/null | grep -oP '"tag_name": "\K[^"]*' | head -1 || echo "")

    if [[ -z "$version" ]]; then
        # Fallback: list releases and get the most recent
        version=$(curl -fsSL "${GITHUB_BASE}/releases.atom" 2>/dev/null | grep -oP '<id>tag:github.com:[^/]+/[^/]+/releases/tag/\K[^<]*' | head -1 || echo "")
    fi

    if [[ -z "$version" ]]; then
        log_warn "Could not determine latest version; using 'latest' tag"
        echo "latest"
    else
        echo "$version"
    fi
}

# Download binary
download_binary() {
    local arch="$1"
    local version="$2"
    local binary_name="voicerouter-${version}-${arch}-unknown-linux-gnu"
    local download_url="${GITHUB_BASE}/releases/download/${version}/${binary_name}"
    local temp_file

    temp_file=$(mktemp)
    trap "rm -f '$temp_file'" EXIT

    log_info "Downloading voicerouter v${version} for ${arch}..."
    log_info "URL: ${download_url}"

    if ! curl -fsSL -o "$temp_file" "$download_url"; then
        log_error "Failed to download binary from:"
        log_error "  ${download_url}"
        log_info "Try building from source:"
        log_info "  git clone https://github.com/${REPO}.git"
        log_info "  cd ygg-voicerouter && cargo build --release"
        exit 1
    fi

    log_success "Downloaded"
    echo "$temp_file"
}

# Install binary
install_binary() {
    local temp_file="$1"

    log_info "Installing to ${INSTALL_DIR}/"

    # Ensure install directory exists
    mkdir -p "$INSTALL_DIR"

    # Copy and make executable
    cp "$temp_file" "${INSTALL_DIR}/voicerouter"
    chmod +x "${INSTALL_DIR}/voicerouter"

    log_success "Installed to ${INSTALL_DIR}/voicerouter"

    # Install overlay binary if present in the same release
    local overlay_file="${temp_file}-overlay"
    local overlay_name="voicerouter-overlay-${version}-${arch}-unknown-linux-gnu"
    local overlay_url="${GITHUB_BASE}/releases/download/${version}/${overlay_name}"
    if curl -fsSL -o "$overlay_file" "$overlay_url" 2>/dev/null; then
        cp "$overlay_file" "${INSTALL_DIR}/voicerouter-overlay"
        chmod +x "${INSTALL_DIR}/voicerouter-overlay"
        log_success "Installed overlay to ${INSTALL_DIR}/voicerouter-overlay"
        rm -f "$overlay_file"
    else
        log_info "Overlay binary not found in release (optional — build from source with GTK4)"
    fi

    # Check if in PATH
    if ! command -v voicerouter &> /dev/null; then
        log_warn "voicerouter is not in your PATH"
        log_info "Add to your shell profile (~/.bashrc, ~/.zshrc, etc):"
        log_info "  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
    fi
}

# Prompt for model download
download_model_prompt() {
    local choice

    mkdir -p "$MODEL_DIR"

    log_info "voicerouter requires the Paraformer speech recognition model."
    log_info "Model directory: ${MODEL_DIR}"
    echo ""

    # Check if model already exists
    if ls "${MODEL_DIR}"/*.onnx &> /dev/null; then
        log_success "Model files detected in ${MODEL_DIR}"
        return 0
    fi

    echo -e "${YELLOW}Model files not found.${NC}"
    echo ""
    echo "Download now? (y/n) [y]: "
    read -r choice || choice="y"

    case "$choice" in
        [yY]|"")
            download_model
            ;;
        *)
            log_info "Skipping model download."
            log_info "Download manually later:"
            log_info "  1. Visit: https://github.com/k2-fsa/sherpa-onnx/releases"
            log_info "  2. Download 'paraformer-zh' or 'paraformer-en' model"
            log_info "  3. Extract to: ${MODEL_DIR}"
            ;;
    esac

    echo ""
    log_info "Tip: cloud ASR is available as an alternative to local models."
    log_info "  DashScope Qwen3-ASR-Flash-Realtime offers streaming recognition with automatic"
    log_info "  fallback to local when offline. To enable, set DASHSCOPE_API_KEY and add:"
    log_info "    [asr.cloud]"
    log_info "    enabled = true"
    log_info "  to ~/.config/voicerouter/config.toml (see INSTALL.md for full setup)."
    log_info "  A local model download is still recommended for offline fallback."
}

# Download model (placeholder)
download_model() {
    log_info "Model download instructions:"
    log_info ""
    log_info "1. Visit: https://github.com/k2-fsa/sherpa-onnx/releases"
    log_info "2. Find 'sherpa-onnx-paraformer-*' (Chinese) or 'sherpa-onnx-paraformer-*' (English)"
    log_info "3. Download the tar.gz file"
    log_info "4. Extract to ${MODEL_DIR}:"
    log_info ""
    log_info "   mkdir -p ${MODEL_DIR}"
    log_info "   tar xzf sherpa-onnx-paraformer-*.tar.gz"
    log_info "   cp -r sherpa-onnx-paraformer-*/* ${MODEL_DIR}/"
    log_info ""
    log_info "For Chinese: Use 'paraformer-zh' or 'paraformer-large-zh'"
    log_info "For English: Use 'paraformer-en'"
    log_info ""
    log_info "Then run: voicerouter setup"
}

# Main installation flow
main() {
    echo ""
    log_info "voicerouter installation script"
    echo ""

    # Detect architecture
    local arch
    arch=$(detect_arch)
    log_info "Detected architecture: ${arch}"

    # Get latest version
    local version
    version=$(get_latest_version)
    log_info "Latest version: ${version}"

    # Download binary
    local temp_file
    temp_file=$(download_binary "$arch" "$version")

    # Install binary
    install_binary "$temp_file"
    rm -f "$temp_file"

    echo ""

    # Prompt for model download
    download_model_prompt

    echo ""
    log_success "Installation complete!"
    echo ""
    log_info "Next steps:"
    log_info "  1. Verify installation: voicerouter --version"
    log_info "  2. Check system: voicerouter setup"
    log_info "  3. Start the daemon: voicerouter"
    log_info ""
    log_info "For more info: https://github.com/${REPO}"
    echo ""
}

main "$@"

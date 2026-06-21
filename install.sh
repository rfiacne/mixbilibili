#!/usr/bin/env bash
set -euo pipefail

PREFIX="/usr/local"
ACTION="install"

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS] [ACTION]

Actions:
  install     Build and install mixbilibili (default)
  uninstall   Remove mixbilibili

Options:
  --prefix PATH   Installation prefix (default: /usr/local)
  -h, --help      Show this help message

Examples:
  $(basename "$0")                    # Install to /usr/local/bin
  $(basename "$0") --prefix ~/.local  # Install to ~/.local/bin
  $(basename "$0") uninstall          # Remove from /usr/local/bin
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            PREFIX="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        install|uninstall)
            ACTION="$1"
            shift
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

BIN_NAME="mixbilibili"
INSTALL_PATH="${PREFIX}/bin/${BIN_NAME}"

if [[ "$ACTION" == "uninstall" ]]; then
    if [[ -f "$INSTALL_PATH" ]]; then
        echo "Removing ${INSTALL_PATH}..."
        sudo rm -f "$INSTALL_PATH"
        echo "Uninstalled ${BIN_NAME}."
    else
        echo "${INSTALL_PATH} not found. Already uninstalled."
    fi
    exit 0
fi

if command -v "$BIN_NAME" &>/dev/null; then
    EXISTING_VERSION=$("$BIN_NAME" --version 2>/dev/null || echo "unknown")
    BUILD_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
    if [[ "$EXISTING_VERSION" == "$BUILD_VERSION" ]]; then
        echo "Version ${EXISTING_VERSION} already installed at $(command -v "$BIN_NAME")."
        echo "Use '$(basename "$0") uninstall' first to reinstall."
        exit 0
    fi
    echo "Updating ${BIN_NAME} from ${EXISTING_VERSION} to ${BUILD_VERSION}..."
fi

echo "Building ${BIN_NAME} in release mode..."
cargo build --release

echo "Installing to ${INSTALL_PATH}..."
sudo mkdir -p "${PREFIX}/bin"
sudo cp target/release/"${BIN_NAME}" "${INSTALL_PATH}"

echo "Done. Version: $("${INSTALL_PATH}" --version)"

#!/usr/bin/env bash
# Builds the PGN Tree Viewer for both Windows and Ubuntu/Linux and
# drops the resulting binaries in dist/.
#
# Requirements (one-time setup):
#   rustup target add x86_64-pc-windows-gnu x86_64-unknown-linux-gnu
#   sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64
#
# Usage:
#   ./build.sh            # build both targets
#   ./build.sh windows     # build only the Windows exe
#   ./build.sh linux       # build only the Linux binary
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

WINDOWS_TARGET="x86_64-pc-windows-gnu"
LINUX_TARGET="x86_64-unknown-linux-gnu"

DIST_DIR="$SCRIPT_DIR/dist"
WINDOWS_OUT="$DIST_DIR/PgnTreeViewer.exe"
LINUX_OUT="$DIST_DIR/PgnTreeViewer-linux"

mkdir -p "$DIST_DIR"

build_windows() {
    echo "==> Building Windows target ($WINDOWS_TARGET)..."
    if ! rustup target list --installed | grep -q "^${WINDOWS_TARGET}\$"; then
        echo "Error: target $WINDOWS_TARGET not installed. Run:" >&2
        echo "  rustup target add $WINDOWS_TARGET" >&2
        exit 1
    fi
    if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
        echo "Error: mingw-w64 linker not found. Run:" >&2
        echo "  sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64" >&2
        exit 1
    fi

    # +crt-static avoids depending on libgcc_s_seh-1.dll / libwinpthread-1.dll,
    # which are not present on a stock Windows install.
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build --release --target "$WINDOWS_TARGET"

    cp "target/$WINDOWS_TARGET/release/pgntreeviewer.exe" "$WINDOWS_OUT"
    echo "==> Windows build ready: $WINDOWS_OUT"
}

build_linux() {
    echo "==> Building Linux target ($LINUX_TARGET)..."
    if ! rustup target list --installed | grep -q "^${LINUX_TARGET}\$"; then
        echo "Error: target $LINUX_TARGET not installed. Run:" >&2
        echo "  rustup target add $LINUX_TARGET" >&2
        exit 1
    fi

    cargo build --release --target "$LINUX_TARGET"

    cp "target/$LINUX_TARGET/release/pgntreeviewer" "$LINUX_OUT"
    chmod +x "$LINUX_OUT"
    echo "==> Linux build ready: $LINUX_OUT"
}

case "${1:-all}" in
    windows) build_windows ;;
    linux)   build_linux ;;
    all)     build_windows; build_linux ;;
    *)
        echo "Usage: $0 [windows|linux|all]" >&2
        exit 1
        ;;
esac

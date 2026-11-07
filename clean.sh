#!/usr/bin/env bash
# Removes build artifacts produced by build.sh / cargo.
#
# Only the built binaries are removed from dist/ — any other handout
# material there (e.g. PDFs) is left untouched.
#
# Usage:
#   ./clean.sh            # remove target/ and the binaries in dist/
#   ./clean.sh target     # remove only target/ (cargo clean)
#   ./clean.sh dist       # remove only the binaries in dist/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DIST_DIR="$SCRIPT_DIR/dist"
WINDOWS_OUT="$DIST_DIR/PgnTreeViewer.exe"
LINUX_OUT="$DIST_DIR/PgnTreeViewer-linux"

clean_target() {
    echo "==> Cleaning cargo target directory..."
    cargo clean
}

clean_dist() {
    echo "==> Removing built binaries from dist/..."
    rm -f "$WINDOWS_OUT" "$LINUX_OUT"
}

case "${1:-all}" in
    target) clean_target ;;
    dist)   clean_dist ;;
    all)    clean_target; clean_dist ;;
    *)
        echo "Usage: $0 [target|dist|all]" >&2
        exit 1
        ;;
esac

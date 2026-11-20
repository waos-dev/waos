#!/usr/bin/env sh
# Installs the newest WaOS and puts `waos` and `cargo-waos` (one binary under two names) on the
# PATH, in one step:
# - macOS (Apple Silicon): WaOS.app into /Applications, or ~/Applications where that can't be
#   written, and the two names linked to the binary inside it.
# - Linux (x86_64): the `waos` binary, which also runs WaOS without a display.
#
# Usage:
#   curl -fsSL https://waos.app/install.sh | sh
#
# Environment variables:
#   WAOS_INSTALL_DIR  Where `waos` and `cargo-waos` go (default: "$HOME/.local/bin")
#   WAOS_SITE         Where the newest release is read from (default: https://waos.app)
set -eu

SITE="${WAOS_SITE:-https://waos.app}"
INSTALL_DIR="${WAOS_INSTALL_DIR:-$HOME/.local/bin}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dir)
            [ "$#" -ge 2 ] || { echo "install.sh: missing value for $1" >&2; exit 2; }
            INSTALL_DIR="$2"
            shift 2
            ;;
        -h|--help)
            cat <<'EOF'
Usage: install.sh [--dir <install-dir>]

Installs the newest WaOS and puts `waos` and `cargo-waos` in <install-dir>
(default: ~/.local/bin).
EOF
            exit 0
            ;;
        *)
            echo "install.sh: unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

command -v curl >/dev/null 2>&1 || { echo "install.sh: curl is required" >&2; exit 1; }

note_path() {
    case ":${PATH:-}:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            printf '\nAdd %s to your PATH to run `waos` and `cargo waos`:\n' "$INSTALL_DIR"
            printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
            ;;
    esac
}

has_vulkan_icd() {
    if [ -n "${VK_ICD_FILENAMES:-}" ] || [ -n "${VK_DRIVER_FILES:-}" ]; then
        return 0
    fi
    for dir in /usr/share/vulkan/icd.d /etc/vulkan/icd.d /usr/local/share/vulkan/icd.d; do
        for icd in "$dir"/*.json; do
            [ -f "$icd" ] && return 0
        done
    done
    return 1
}

# The newest release's version, from the site's reading of GitHub.
newest_version() {
    curl -fsSL "$SITE/api/latest" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p'
}

# Downloads the newest release's file for $1 (macos, linux) into $2.
download() {
    printf 'Downloading WaOS %s for %s...\n' "$VERSION" "$1"
    curl -fL --progress-bar "$SITE/download/$1" -o "$2"
}

link_into_install_dir() {
    mkdir -p "$INSTALL_DIR"
    ln -sf "$1" "$INSTALL_DIR/waos"
    ln -sf "$1" "$INSTALL_DIR/cargo-waos"
}

VERSION="$(newest_version)"
[ -n "$VERSION" ] || { echo "install.sh: could not read the newest WaOS from $SITE/api/latest" >&2; exit 1; }
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

case "$(uname -s)/$(uname -m)" in
    Darwin/arm64)
        download macos "$WORK/WaOS.dmg"
        if [ -w /Applications ]; then APPLICATIONS=/Applications; else APPLICATIONS="$HOME/Applications"; fi
        mkdir -p "$APPLICATIONS"
        # A running WaOS keeps its old binary: ask it to quit before it is replaced.
        if pgrep -xq waos; then
            osascript -e 'quit app id "dev.waos.app"' >/dev/null 2>&1 || true
            sleep 2
        fi
        MOUNT="$WORK/mount"
        mkdir -p "$MOUNT"
        hdiutil attach -quiet -nobrowse -readonly -mountpoint "$MOUNT" "$WORK/WaOS.dmg"
        rm -rf "$APPLICATIONS/WaOS.app"
        cp -R "$MOUNT/WaOS.app" "$APPLICATIONS/WaOS.app"
        hdiutil detach -quiet "$MOUNT"
        link_into_install_dir "$APPLICATIONS/WaOS.app/Contents/MacOS/waos"
        printf 'Installed WaOS %s in %s, with %s and %s.\n' \
            "$VERSION" "$APPLICATIONS" "$INSTALL_DIR/waos" "$INSTALL_DIR/cargo-waos"
        ;;
    Linux/x86_64|Linux/amd64)
        command -v tar >/dev/null 2>&1 || { echo "install.sh: tar is required" >&2; exit 1; }
        download linux "$WORK/waos.tar.gz"
        tar -xzf "$WORK/waos.tar.gz" -C "$WORK"
        [ -f "$WORK/waos" ] || { echo "install.sh: the download held no waos binary" >&2; exit 1; }
        mkdir -p "$INSTALL_DIR"
        install -m 755 "$WORK/waos" "$INSTALL_DIR/waos"
        ln -sf waos "$INSTALL_DIR/cargo-waos"
        printf 'Installed WaOS %s as %s and %s.\n' "$VERSION" "$INSTALL_DIR/waos" "$INSTALL_DIR/cargo-waos"
        if ! has_vulkan_icd; then
            cat <<'EOF'
Note: no Vulkan driver was found. WaOS without a display draws through Vulkan; Mesa's lavapipe
works without a GPU:
  sudo apt-get install -y mesa-vulkan-drivers
EOF
        fi
        ;;
    *)
        cat >&2 <<EOF
install.sh: WaOS is released for macOS on Apple Silicon and Linux on x86_64 (this is $(uname -s) on $(uname -m)).
Elsewhere, the command alone, for building and packaging:  cargo install waos-cli
EOF
        exit 1
        ;;
esac
note_path

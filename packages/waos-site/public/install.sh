#!/usr/bin/env sh
# Install the unified `waos` binary (and its `cargo-waos` symlink) into ~/.local/bin.
#
# Usage:
#   curl -fsSL https://waos.app/install.sh | sh
#
# Environment variables:
#   WAOS_INSTALL_DIR  Destination directory (default: "$HOME/.local/bin")
#   WAOS_VERSION      Release version to install, e.g. 0.4.0 or v0.4.0 (default: latest)
#   WAOS_REPO         GitHub repository (default: waos-dev/waos)
set -eu

REPO="${WAOS_REPO:-waos-dev/waos}"
INSTALL_DIR="${WAOS_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${WAOS_VERSION:-}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        -v|--version)
            [ "$#" -ge 2 ] || { echo "install.sh: missing value for $1" >&2; exit 2; }
            VERSION="$2"
            shift 2
            ;;
        --dir)
            [ "$#" -ge 2 ] || { echo "install.sh: missing value for $1" >&2; exit 2; }
            INSTALL_DIR="$2"
            shift 2
            ;;
        -h|--help)
            cat <<'EOF'
Usage: install.sh [--version <version>] [--dir <install-dir>]

Installs the unified `waos` CLI/runtime binary and creates a `cargo-waos`
symlink beside it (default: ~/.local/bin).
EOF
            exit 0
            ;;
        *)
            echo "install.sh: unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

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
        if [ -d "$dir" ]; then
            for icd in "$dir"/*.json; do
                if [ -f "$icd" ]; then
                    return 0
                fi
            done
        fi
    done
    return 1
}

resolve_latest_version() {
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" || true)
    tag="${url##*/tag/}"
    if [ -n "$tag" ] && [ "$tag" != "$url" ]; then
        printf '%s' "${tag#v}"
        return 0
    fi
    tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        | head -n 1)
    if [ -n "$tag" ]; then
        printf '%s' "${tag#v}"
        return 0
    fi
    return 1
}

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64|amd64)
                ASSET_ARCH="x86_64"
                ;;
            *)
                echo "install.sh: prebuilt Linux tarballs are currently published for x86_64 (found $ARCH)." >&2
                echo "Build from source with: cargo build --release -p waos-app --bin waos" >&2
                exit 1
                ;;
        esac

        command -v curl >/dev/null 2>&1 || { echo "install.sh: curl is required" >&2; exit 1; }
        command -v tar >/dev/null 2>&1 || { echo "install.sh: tar is required" >&2; exit 1; }

        if [ -z "$VERSION" ]; then
            VERSION="$(resolve_latest_version)" || {
                echo "install.sh: could not determine the latest WaOS release from GitHub" >&2
                exit 1
            }
        else
            VERSION="${VERSION#v}"
        fi

        TAG="v$VERSION"
        ARCHIVE="waos-${VERSION}-linux-${ASSET_ARCH}.tar.gz"
        URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"

        TMPDIR_INSTALL="$(mktemp -d)"
        trap 'rm -rf "$TMPDIR_INSTALL"' EXIT

        printf 'Downloading %s...\n' "$URL"
        curl -fsSL "$URL" -o "$TMPDIR_INSTALL/$ARCHIVE"
        tar -xzf "$TMPDIR_INSTALL/$ARCHIVE" -C "$TMPDIR_INSTALL"

        if [ ! -f "$TMPDIR_INSTALL/waos" ]; then
            echo "install.sh: archive $ARCHIVE did not contain a waos binary" >&2
            exit 1
        fi

        mkdir -p "$INSTALL_DIR"
        install -m 755 "$TMPDIR_INSTALL/waos" "$INSTALL_DIR/waos"
        ln -sf waos "$INSTALL_DIR/cargo-waos"

        printf 'Installed %s and %s (WaOS %s).\n' "$INSTALL_DIR/waos" "$INSTALL_DIR/cargo-waos" "$VERSION"

        if ! has_vulkan_icd; then
            cat <<'EOF'

Note: no Vulkan ICD was found in /usr/share/vulkan/icd.d or /etc/vulkan/icd.d.
Headless Linux rendering needs a Vulkan driver (Mesa lavapipe works without a GPU):
  sudo apt-get install -y mesa-vulkan-drivers
EOF
        fi

        note_path
        ;;

    Darwin)
        APP_BIN=""
        for candidate in \
            "/Applications/WaOS.app/Contents/MacOS/waos" \
            "$HOME/Applications/WaOS.app/Contents/MacOS/waos"
        do
            if [ -x "$candidate" ]; then
                APP_BIN="$candidate"
                break
            fi
        done

        if [ -n "$APP_BIN" ]; then
            mkdir -p "$INSTALL_DIR"
            ln -sf "$APP_BIN" "$INSTALL_DIR/waos"
            ln -sf "$APP_BIN" "$INSTALL_DIR/cargo-waos"
            printf 'Linked %s and %s -> %s.\n' "$INSTALL_DIR/waos" "$INSTALL_DIR/cargo-waos" "$APP_BIN"
            note_path
        else
            cat <<EOF
WaOS.app was not found in /Applications or ~/Applications.

Download WaOS.dmg from https://waos.app or https://github.com/${REPO}/releases/latest,
drag WaOS.app into /Applications, and re-run this script to link \`waos\` and \`cargo-waos\`.
EOF
            exit 1
        fi
        ;;

    *)
        echo "install.sh: unsupported operating system: $OS" >&2
        echo "Download WaOS from https://waos.app." >&2
        exit 1
        ;;
esac

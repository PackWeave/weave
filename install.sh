#!/bin/sh
# install.sh — Install the weave binary from GitHub Releases
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/PackWeave/weave/main/install.sh | sh
#
# Environment variables:
#   WEAVE_INSTALL_DIR  Override the install directory (default: /usr/local/bin or ~/.local/bin)

set -eu

REPO="PackWeave/weave"
BINARY="weave"
GITHUB_API="https://api.github.com/repos/${REPO}/releases/latest"
GITHUB_RELEASES="https://github.com/${REPO}/releases/download"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

info()  { printf '\033[0;34m[weave]\033[0m %s\n' "$*"; }
ok()    { printf '\033[0;32m[weave]\033[0m %s\n' "$*"; }
err()   { printf '\033[0;31m[weave] error:\033[0m %s\n' "$*" >&2; }
die()   { err "$*"; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found. Please install it and try again."
}

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------

detect_platform() {
    _os="$(uname -s)"
    _arch="$(uname -m)"

    case "$_os" in
        Darwin)
            _os_name="apple-darwin"
            ;;
        Linux)
            _os_name="unknown-linux-musl"
            ;;
        *)
            die "Unsupported operating system: $_os. Only macOS and Linux are supported."
            ;;
    esac

    case "$_arch" in
        x86_64 | amd64)
            _arch_name="x86_64"
            ;;
        aarch64 | arm64)
            _arch_name="aarch64"
            ;;
        *)
            die "Unsupported CPU architecture: $_arch. Only x86_64 and arm64 are supported."
            ;;
    esac

    TARGET="${_arch_name}-${_os_name}"
}

# ---------------------------------------------------------------------------
# Resolve the latest release version from the GitHub API
# ---------------------------------------------------------------------------

fetch_latest_version() {
    info "Fetching latest release version..."
    VERSION="$(curl -fsSL "${GITHUB_API}" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
    if [ -z "$VERSION" ]; then
        die "Could not determine the latest release version. Check your internet connection or visit https://github.com/${REPO}/releases."
    fi
    # Strip a leading 'v' if present so asset names stay consistent
    VERSION_NUM="${VERSION#v}"
    info "Latest version: ${VERSION}"
}

# ---------------------------------------------------------------------------
# Choose install directory
# ---------------------------------------------------------------------------

choose_install_dir() {
    if [ -n "${WEAVE_INSTALL_DIR:-}" ]; then
        INSTALL_DIR="$WEAVE_INSTALL_DIR"
        info "Using WEAVE_INSTALL_DIR: ${INSTALL_DIR}"
        return
    fi

    if [ -w "/usr/local/bin" ]; then
        INSTALL_DIR="/usr/local/bin"
    else
        INSTALL_DIR="${HOME}/.local/bin"
        info "/usr/local/bin is not writable; falling back to ${INSTALL_DIR}"
        mkdir -p "${INSTALL_DIR}"

        # Warn if ~/.local/bin is not on PATH
        case ":${PATH}:" in
            *":${INSTALL_DIR}:"*) ;;
            *)
                printf '\033[0;33m[weave] warning:\033[0m %s is not on your PATH.\n' "${INSTALL_DIR}" >&2
                printf '        Add the following line to your shell profile (~/.bashrc, ~/.zshrc, etc.):\n' >&2
                printf '          export PATH="%s:$PATH"\n' "${INSTALL_DIR}" >&2
                ;;
        esac
    fi
}

# ---------------------------------------------------------------------------
# Download, verify, and install
# ---------------------------------------------------------------------------

download_and_install() {
    ASSET_NAME="${BINARY}-${VERSION_NUM}-${TARGET}.tar.gz"
    DOWNLOAD_URL="${GITHUB_RELEASES}/${VERSION}/${ASSET_NAME}"
    CHECKSUM_URL="${GITHUB_RELEASES}/${VERSION}/${ASSET_NAME}.sha256"

    TMPDIR="$(mktemp -d)"
    # Ensure the temp directory is removed on exit
    # shellcheck disable=SC2064
    trap "rm -rf '${TMPDIR}'" EXIT

    ARCHIVE="${TMPDIR}/${ASSET_NAME}"

    info "Downloading ${ASSET_NAME}..."
    if ! curl -fsSL --fail "${DOWNLOAD_URL}" -o "${ARCHIVE}"; then
        die "Download failed. Could not fetch: ${DOWNLOAD_URL}\nCheck that version ${VERSION} has a release asset for your platform (${TARGET})."
    fi

    # Verify SHA256 checksum if a checksum file is available
    CHECKSUM_FILE="${TMPDIR}/${ASSET_NAME}.sha256"
    if curl -fsSL --fail "${CHECKSUM_URL}" -o "${CHECKSUM_FILE}" 2>/dev/null; then
        info "Verifying SHA256 checksum..."
        _expected="$(awk '{print $1}' "${CHECKSUM_FILE}")"

        # sha256sum (Linux) or shasum -a 256 (macOS)
        if command -v sha256sum >/dev/null 2>&1; then
            _actual="$(sha256sum "${ARCHIVE}" | awk '{print $1}')"
        elif command -v shasum >/dev/null 2>&1; then
            _actual="$(shasum -a 256 "${ARCHIVE}" | awk '{print $1}')"
        else
            err "Neither sha256sum nor shasum found; skipping checksum verification."
            _actual="$_expected"
        fi

        if [ "$_actual" != "$_expected" ]; then
            die "Checksum mismatch!\n  Expected: ${_expected}\n  Got:      ${_actual}\nThe downloaded file may be corrupt. Please try again."
        fi
        ok "Checksum verified."
    else
        info "No checksum file found at release; skipping verification."
    fi

    info "Extracting archive..."
    tar -xzf "${ARCHIVE}" -C "${TMPDIR}"

    # The binary may be at the root or inside a subdirectory matching the asset name without .tar.gz
    EXTRACTED_BINARY="${TMPDIR}/${BINARY}"
    if [ ! -f "${EXTRACTED_BINARY}" ]; then
        # Try one directory level deep
        EXTRACTED_BINARY="$(find "${TMPDIR}" -name "${BINARY}" -type f | head -n 1)"
        if [ -z "${EXTRACTED_BINARY}" ]; then
            die "Could not find '${BINARY}' binary in the extracted archive. The release asset layout may have changed."
        fi
    fi

    info "Installing ${BINARY} to ${INSTALL_DIR}..."
    chmod +x "${EXTRACTED_BINARY}"
    cp "${EXTRACTED_BINARY}" "${INSTALL_DIR}/${BINARY}"

    ok "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
}

# ---------------------------------------------------------------------------
# Confirm the installation works
# ---------------------------------------------------------------------------

verify_install() {
    INSTALLED_BIN="${INSTALL_DIR}/${BINARY}"

    if [ ! -x "${INSTALLED_BIN}" ]; then
        die "Installation check failed: ${INSTALLED_BIN} is not executable."
    fi

    info "Running '${BINARY} --version' to confirm install..."
    if "${INSTALLED_BIN}" --version; then
        ok "Installation successful!"
    else
        die "'${BINARY} --version' failed. The binary may not be compatible with your system."
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    need curl
    need tar

    detect_platform
    fetch_latest_version
    choose_install_dir
    download_and_install
    verify_install
}

main "$@"

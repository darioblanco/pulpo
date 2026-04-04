#!/usr/bin/env bash
set -euo pipefail

BIN_DIR=${BIN_DIR:-/usr/local/bin}
REPO=${REPO:-darioblanco/pulpo}
TARGET=${TARGET:-}

info() {
  printf '[pulpo install] %s\n' "$1"
}

error_exit() {
  printf '[pulpo install] ERROR: %s\n' "$1" >&2
  exit 1
}

detect_target() {
  local uname_s
  local uname_m
  uname_s=$(uname -s)
  uname_m=$(uname -m)

  local os
  case "$uname_s" in
    Linux) os=unknown-linux-gnu ;;
    Darwin) os=apple-darwin ;;
    *) error_exit "unsupported OS: $uname_s" ;;
  esac

  local arch
  case "$uname_m" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) error_exit "unsupported architecture: $uname_m" ;;
  esac

  printf '%s-%s' "$arch" "$os"
}

if [ -z "$TARGET" ]; then
  TARGET=$(detect_target)
  info "Auto-detected target: $TARGET"
else
  info "Using overridden target: $TARGET"
fi

ASSET="pulpod-${TARGET}.tar.xz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"

TMP_DIR=$(mktemp -d 2>/dev/null || mktemp -d -t pulpo-install)
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

info "Downloading Pulpo release from ${DOWNLOAD_URL}"
curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/${ASSET}"

tar -xf "$TMP_DIR/${ASSET}" -C "$TMP_DIR"

for binary in pulpod pulpo; do
  if [ ! -x "$TMP_DIR/$binary" ]; then
    error_exit "binary $binary not found in ${ASSET}"
  fi
done

install_binary() {
  local binary=$1
  local dest_dir="$BIN_DIR"
  local src="$TMP_DIR/$binary"

  if [ ! -d "$dest_dir" ]; then
    info "Creating $dest_dir"
    sudo mkdir -p "$dest_dir"
  fi

  if [ -w "$dest_dir" ]; then
    install -m 0755 "$src" "$dest_dir/$binary"
  else
    sudo install -m 0755 "$src" "$dest_dir/$binary"
  fi
}

for binary in pulpod pulpo; do
  info "Installing $binary to $BIN_DIR"
  install_binary "$binary"
done

info "Pulpo is installed. Re-run the script to upgrade (it always downloads the latest release)."

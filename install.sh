#!/bin/sh
# csshd installer (Linux / macOS).
#
# Detects OS+arch, downloads the matching release artifact from
# github.com/css-md/csshd, verifies the SHA256 checksum, and drops
# the binary into ~/.local/bin (creating it if necessary).
#
# Usage (from a release URL):
#   curl --proto '=https' --tlsv1.2 -LsSf \
#     https://github.com/css-md/csshd/releases/latest/download/csshd-installer.sh | sh
#
# This file uses the placeholder __TAG__ which the release workflow
# replaces with the actual git tag (e.g. v0.1.0) before publishing.

set -eu

REPO="css-md/csshd"
TAG="__TAG__"
INSTALL_DIR="${CSSHD_INSTALL_DIR:-$HOME/.local/bin}"

uname_s=$(uname -s)
uname_m=$(uname -m)

case "$uname_s" in
  Linux)  os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) printf 'csshd: unsupported OS: %s\n' "$uname_s" >&2; exit 1 ;;
esac

case "$uname_m" in
  x86_64|amd64) arch="x86_64" ;;
  arm64|aarch64) arch="aarch64" ;;
  *) printf 'csshd: unsupported arch: %s\n' "$uname_m" >&2; exit 1 ;;
esac

target="${arch}-${os}"
version="${TAG#v}"
stem="csshd-${version}-${target}"
archive="${stem}.tar.gz"
url="https://github.com/${REPO}/releases/download/${TAG}/${archive}"
sha_url="${url}.sha256"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

printf 'csshd: downloading %s\n' "$url"
curl --proto '=https' --tlsv1.2 -fLsS "$url" -o "$tmp/$archive"
curl --proto '=https' --tlsv1.2 -fLsS "$sha_url" -o "$tmp/$archive.sha256" || true

if [ -f "$tmp/$archive.sha256" ]; then
  printf 'csshd: verifying checksum\n'
  expected=$(awk '{print $1}' "$tmp/$archive.sha256")
  if command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
  else
    actual=$(sha256sum "$tmp/$archive" | awk '{print $1}')
  fi
  if [ "$expected" != "$actual" ]; then
    printf 'csshd: checksum mismatch (expected %s, got %s)\n' "$expected" "$actual" >&2
    exit 1
  fi
fi

mkdir -p "$INSTALL_DIR"
( cd "$tmp" && tar -xzf "$archive" )
mv "$tmp/$stem/csshd" "$INSTALL_DIR/csshd"
chmod +x "$INSTALL_DIR/csshd"

printf '\ncsshd: installed → %s/csshd\n' "$INSTALL_DIR"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf '\nNote: %s is not on your PATH. Add it with:\n' "$INSTALL_DIR"
    printf '    echo '\''export PATH="%s:$PATH"'\'' >> ~/.bashrc   # or ~/.zshrc\n' "$INSTALL_DIR"
    ;;
esac

printf '\nNext: csshd login --helpdesk https://your-helpdesk-url\n'

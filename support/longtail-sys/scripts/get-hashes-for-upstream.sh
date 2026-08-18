#!/bin/sh

VER="v0.4.3"
DL_PATH=$(mktemp -d)
DL_URL="https://github.com/DanEngelbrecht/longtail/releases/download/${VER}/"
DL_EXT=".zip"

PLATFORMS="darwin-arm64
darwin-x64
linux-x64
win32-x64
"

echo "Downloading from ${DL_URL} to ${DL_PATH}"
printf "%s" "$PLATFORMS" |
  while IFS='' read -r PLATFORM; do
    FILE="${PLATFORM}${DL_EXT}"
    curl -Lqs "${DL_URL}${FILE}" -o "${DL_PATH}/${FILE}"
    sha256sum "${DL_PATH}/${FILE}"
  done

rm -r "${DL_PATH}"

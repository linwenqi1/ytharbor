#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${1:-x86_64-unknown-linux-gnu}"
mkdir -p "$ROOT_DIR/src-tauri/binaries" "$ROOT_DIR/src-tauri/resources/ffmpeg" "$ROOT_DIR/src-tauri/resources/deno"

case "$TARGET" in
  aarch64-*) YTDLP_ASSET="yt-dlp_linux_aarch64" ; DENO_TARGET="aarch64-unknown-linux-gnu" ;;
  *) YTDLP_ASSET="yt-dlp_linux" ; DENO_TARGET="x86_64-unknown-linux-gnu" ;;
esac
curl -fL "https://github.com/yt-dlp/yt-dlp/releases/latest/download/$YTDLP_ASSET" -o "$ROOT_DIR/src-tauri/binaries/yt-dlp-$TARGET"
chmod +x "$ROOT_DIR/src-tauri/binaries/yt-dlp-$TARGET"

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT
curl -fL "https://github.com/denoland/deno/releases/latest/download/deno-$DENO_TARGET.zip" -o "$TEMP_DIR/deno.zip"
unzip -oq "$TEMP_DIR/deno.zip" -d "$ROOT_DIR/src-tauri/resources/deno"
chmod +x "$ROOT_DIR/src-tauri/resources/deno/deno"

if command -v ffmpeg >/dev/null && command -v ffprobe >/dev/null; then
  cp "$(command -v ffmpeg)" "$ROOT_DIR/src-tauri/resources/ffmpeg/ffmpeg"
  cp "$(command -v ffprobe)" "$ROOT_DIR/src-tauri/resources/ffmpeg/ffprobe"
  echo "Copied system FFmpeg binaries. Verify their shared-library dependencies before distribution."
else
  echo "FFmpeg was not found. Add static ffmpeg and ffprobe binaries to src-tauri/resources/ffmpeg." >&2
  exit 1
fi

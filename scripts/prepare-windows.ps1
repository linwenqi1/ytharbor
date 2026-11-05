param(
  [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$BinaryDir = Join-Path $Root "src-tauri\binaries"
$FfmpegDir = Join-Path $Root "src-tauri\resources\ffmpeg"
$DenoDir = Join-Path $Root "src-tauri\resources\deno"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("ytharbor-" + [guid]::NewGuid())

New-Item -ItemType Directory -Force -Path $BinaryDir, $FfmpegDir, $DenoDir, $TempDir | Out-Null
try {
  Write-Host "Downloading yt-dlp..."
  $YtDlpAsset = if ($Target.StartsWith("aarch64")) { "yt-dlp_arm64.exe" } else { "yt-dlp.exe" }
  Invoke-WebRequest "https://github.com/yt-dlp/yt-dlp/releases/latest/download/$YtDlpAsset" -OutFile (Join-Path $BinaryDir "yt-dlp-$Target.exe")

  Write-Host "Downloading FFmpeg essentials build..."
  $Archive = Join-Path $TempDir "ffmpeg.zip"
  Invoke-WebRequest "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip" -OutFile $Archive
  Expand-Archive $Archive -DestinationPath (Join-Path $TempDir "ffmpeg")
  $Ffmpeg = Get-ChildItem (Join-Path $TempDir "ffmpeg") -Recurse -Filter "ffmpeg.exe" | Select-Object -First 1
  $Ffprobe = Get-ChildItem (Join-Path $TempDir "ffmpeg") -Recurse -Filter "ffprobe.exe" | Select-Object -First 1
  if (-not $Ffmpeg -or -not $Ffprobe) { throw "FFmpeg archive layout was not recognized" }
  Copy-Item $Ffmpeg.FullName (Join-Path $FfmpegDir "ffmpeg.exe") -Force
  Copy-Item $Ffprobe.FullName (Join-Path $FfmpegDir "ffprobe.exe") -Force

  Write-Host "Downloading Deno JavaScript runtime for YouTube extraction..."
  $DenoArchive = Join-Path $TempDir "deno.zip"
  Invoke-WebRequest "https://github.com/denoland/deno/releases/latest/download/deno-$Target.zip" -OutFile $DenoArchive
  Expand-Archive $DenoArchive -DestinationPath $DenoDir -Force
  Write-Host "Download core prepared for $Target"
}
finally {
  Remove-Item $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}

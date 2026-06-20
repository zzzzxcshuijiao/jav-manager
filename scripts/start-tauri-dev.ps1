param(
  [switch]$Help,
  [switch]$KillStale,
  [switch]$ExternalVite
)

$ErrorActionPreference = "Stop"
Set-Location (Resolve-Path (Join-Path $PSScriptRoot ".."))

# NOTE: run this from an interactive desktop terminal, NOT from an unattended
# Codex shell session. The Tauri binary opens a WebView2 window; in a session
# without an interactive desktop, window/WebView2 init fails and the process
# exits with 0xffffffff (no Rust panic), which can also destabilize the host.
#
# Default mode = one command does everything: `tauri dev` spawns Vite itself via
# beforeDevCommand (npm run dev), waits for it on :1420, builds the Rust app,
# and tears Vite down when it exits. Use this unless you need two terminals.
#
# -ExternalVite = you already started `npm run dev` in another terminal; this
# disables Tauri's own beforeDevCommand and reuses your running Vite on :1420.

$msvc = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207"
$sdk = "C:\Program Files (x86)\Windows Kits\10"
$sdkver = "10.0.26100.0"

$env:PATH = "$env:USERPROFILE\.cargo\bin;$msvc\bin\Hostx64\x64;$sdk\bin\$sdkver\x64;$env:PATH"
$env:LIB = "$msvc\lib\x64;$sdk\Lib\$sdkver\um\x64;$sdk\Lib\$sdkver\ucrt\x64;$env:LIB"
$env:INCLUDE = "$msvc\include;$sdk\Include\$sdkver\um;$sdk\Include\$sdkver\ucrt;$sdk\Include\$sdkver\shared;$env:INCLUDE"

# Clear stale locks: leftover media-manager.exe holds the cargo target lock and
# leftover vite holds port 1420 (strictPort), both of which break `tauri dev`.
Get-Process media-manager -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Host "Stopping leftover $($_.ProcessName) (PID $($_.Id))"
  Stop-Process -Id $_.Id -Force
}
$conn = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
if ($conn) {
  if ($ExternalVite) {
    Write-Host "Reusing existing dev server on port 1420 (PID $(($conn.OwningProcess | Select-Object -Unique) -join ','))."
  } elseif ($KillStale) {
    $conn | Select-Object -ExpandProperty OwningProcess -Unique | ForEach-Object {
      Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue
      Write-Host "Killed stale holder of port 1420 (PID $_)"
    }
    Start-Sleep -Milliseconds 500
  } else {
    $pids = (($conn.OwningProcess | Select-Object -Unique) -join ',')
    Write-Error "Port 1420 is already in use (PID $pids). Re-run with -KillStale to clear it, or with -ExternalVite to reuse an already-running dev server."
    exit 1
  }
}

if ($Help) {
  & .\node_modules\.bin\tauri.cmd dev --help
  exit $LASTEXITCODE
}

if ($ExternalVite) {
  # Disable Tauri's own beforeDevCommand so it reuses your already-running Vite.
  $configOverridePath = Join-Path $env:TEMP "media-manager-tauri-dev.config.json"
  Set-Content -LiteralPath $configOverridePath -Encoding UTF8 -Value '{"build":{"beforeDevCommand":""}}'
  Write-Host "Launching Tauri with -ExternalVite (beforeDevCommand disabled, reusing your Vite)..."
  & .\node_modules\.bin\tauri.cmd dev --config $configOverridePath
  exit $LASTEXITCODE
}

Write-Host "Launching Tauri dev (will start Vite automatically via beforeDevCommand)..."
& .\node_modules\.bin\tauri.cmd dev
exit $LASTEXITCODE

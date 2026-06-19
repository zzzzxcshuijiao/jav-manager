param(
  [switch]$Help
)

$ErrorActionPreference = "Stop"

Set-Location (Resolve-Path (Join-Path $PSScriptRoot ".."))

$msvc = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207"
$sdk = "C:\Program Files (x86)\Windows Kits\10"
$sdkver = "10.0.26100.0"

$env:PATH = "$env:USERPROFILE\.cargo\bin;$msvc\bin\Hostx64\x64;$sdk\bin\$sdkver\x64;$env:PATH"
$env:LIB = "$msvc\lib\x64;$sdk\Lib\$sdkver\um\x64;$sdk\Lib\$sdkver\ucrt\x64;$env:LIB"
$env:INCLUDE = "$msvc\include;$sdk\Include\$sdkver\um;$sdk\Include\$sdkver\ucrt;$sdk\Include\$sdkver\shared;$env:INCLUDE"

$configOverridePath = Join-Path $env:TEMP "media-manager-tauri-dev.config.json"
Set-Content -LiteralPath $configOverridePath -Encoding UTF8 -Value '{"build":{"beforeDevCommand":""}}'
if ($Help) {
  & .\node_modules\.bin\tauri.cmd dev --help
  exit $LASTEXITCODE
}

& .\node_modules\.bin\tauri.cmd dev --config $configOverridePath

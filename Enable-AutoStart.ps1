$ErrorActionPreference = "Stop"
$RegPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$AppName = "CyNotch"
$AppPath = Join-Path $PSScriptRoot "cynotch.exe"

if (-not (Test-Path -LiteralPath $AppPath -PathType Leaf)) {
    Write-Error "cynotch.exe not found at $AppPath. Place this script in the same folder as cynotch.exe."
    exit 1
}

if (-not (Test-Path -LiteralPath $RegPath)) {
    New-Item -Path $RegPath -Force -ErrorAction Stop | Out-Null
}

Set-ItemProperty -LiteralPath $RegPath -Name $AppName -Value "`"$AppPath`"" -Type String -ErrorAction Stop
Write-Host "CyNotch has been registered to launch automatically on Windows startup!" -ForegroundColor Green
Write-Host "Location: $AppPath"

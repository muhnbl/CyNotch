$RegPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$AppName = "CyNotch"
$AppPath = Join-Path $PSScriptRoot "cynotch.exe"

if (-not (Test-Path $AppPath)) {
    Write-Error "cynotch.exe not found at $AppPath. Place this script in the same folder as cynotch.exe."
    exit 1
}

Set-ItemProperty -Path $RegPath -Name $AppName -Value "`"$AppPath`""
Write-Host "CyNotch has been registered to launch automatically on Windows startup!" -ForegroundColor Green
Write-Host "Location: $AppPath"

$RegPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$AppName = "CyNotch"

Remove-ItemProperty -Path $RegPath -Name $AppName -ErrorAction SilentlyContinue
Write-Host "ℹ️ CyNotch autostart on Windows startup has been removed." -ForegroundColor Yellow

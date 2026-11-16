$RegPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$AppName = "CyNotch"
$AppPath = "$env:LOCALAPPDATA\..\.local\bin\cynotch.exe"

Set-ItemProperty -Path $RegPath -Name $AppName -Value $AppPath
Write-Host "✅ CyNotch has been registered to launch automatically on Windows startup!" -ForegroundColor Green
Write-Host "Location: $AppPath"

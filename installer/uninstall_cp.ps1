$ErrorActionPreference = 'Stop'
$guid = '{E0A8C5B2-9F3D-4E7A-B1C6-8D2F5A3E9B70}'
$target = Join-Path $env:WINDIR 'System32\UnlockProvider.dll'

Remove-Item -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$guid" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "HKLM:\SOFTWARE\Classes\CLSID\$guid" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue

if (Test-Path -LiteralPath $target) {
    Write-Host 'WARN: DLL is in use and will need to be removed after reboot.'
}

Write-Host 'OK'
exit 0

$ErrorActionPreference = 'Stop'
$guid = '{E0A8C5B2-9F3D-4E7A-B1C6-8D2F5A3E9B70}'
$source = Join-Path $PSScriptRoot 'UnlockProvider.dll'
$target = Join-Path $env:WINDIR 'System32\UnlockProvider.dll'

if (-not (Test-Path -LiteralPath $source)) {
    throw "UnlockProvider.dll not found: $source"
}

function Install-ProviderDll {
    param([string]$Source, [string]$Target)
    try {
        Copy-Item -LiteralPath $Source -Destination $Target -Force -ErrorAction Stop
        return
    } catch {
        $stage = "$Target.new"
        $backup = "$Target.old"
        Copy-Item -LiteralPath $Source -Destination $stage -Force -ErrorAction Stop
        Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
        Move-Item -LiteralPath $Target -Destination $backup -Force -ErrorAction Stop
        try {
            Move-Item -LiteralPath $stage -Destination $Target -Force -ErrorAction Stop
        } catch {
            Move-Item -LiteralPath $backup -Destination $Target -Force -ErrorAction Stop
            Remove-Item -LiteralPath $stage -Force -ErrorAction SilentlyContinue
            throw
        }
    }
}

Install-ProviderDll -Source $source -Target $target

$clsid = "HKLM:\SOFTWARE\Classes\CLSID\$guid"
New-Item -Path $clsid -Force | Out-Null
Set-ItemProperty -Path $clsid -Name '(Default)' -Value 'UnlockProvider'
New-Item -Path "$clsid\InprocServer32" -Force | Out-Null
Set-ItemProperty -Path "$clsid\InprocServer32" -Name '(Default)' -Value $target
Set-ItemProperty -Path "$clsid\InprocServer32" -Name 'ThreadingModel' -Value 'Apartment'

$cp = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$guid"
New-Item -Path $cp -Force | Out-Null
Set-ItemProperty -Path $cp -Name '(Default)' -Value 'UnlockProvider'

Write-Host 'OK'
exit 0

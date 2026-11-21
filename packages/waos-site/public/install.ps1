# Installs the newest WaOS on Windows (x64) and puts its command, `waos`, on the user's PATH, in one
# step: the release's installer, run silently for this user alone, and its folder on the PATH.
#
# Usage, in PowerShell:
#   irm https://waos.app/install.ps1 | iex
#
# Environment variables:
#   WAOS_SITE  Where the newest release is read from (default: https://waos.app)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$site = if ($env:WAOS_SITE) { $env:WAOS_SITE } else { 'https://waos.app' }

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'WaOS is released for 64-bit Windows.'
}

$version = (Invoke-RestMethod "$site/api/latest").version
$work = Join-Path ([IO.Path]::GetTempPath()) ("waos-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $setup = Join-Path $work 'WaOS-setup.exe'
    Write-Host "Downloading WaOS $version for Windows..."
    Invoke-WebRequest "$site/download/windows" -OutFile $setup

    # A running WaOS keeps its files open: ask it to stop before they are replaced.
    Get-Process waos -ErrorAction SilentlyContinue | Stop-Process -ErrorAction SilentlyContinue
    $installer = Start-Process $setup -ArgumentList '/S' -Wait -PassThru
    if ($installer.ExitCode -ne 0) { throw "the installer stopped with code $($installer.ExitCode)" }

    # Where the installer put WaOS, as `waos` reads it too.
    $entry = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\WaOS'
    $folder = $null
    foreach ($hive in 'HKCU:', 'HKLM:') {
        $location = (Get-ItemProperty "$hive\$entry" -ErrorAction SilentlyContinue).InstallLocation
        if ($location) { $folder = $location.Trim('"'); break }
    }
    if (-not $folder -or -not (Test-Path (Join-Path $folder 'waos.exe'))) {
        throw 'the installer ran, but WaOS is not listed among the installed apps'
    }

    $path = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($path -split ';') -contains $folder)) {
        $joined = (@($path, $folder) | Where-Object { $_ }) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $joined, 'User')
        $env:Path = "$env:Path;$folder"
        Write-Host "Added $folder to your PATH; open a new terminal for other shells to see it."
    }
    Write-Host "Installed WaOS $version in $folder, with its command, waos."
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

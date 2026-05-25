param(
    [string]$Version = "latest",
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "fvCapture"),
    [switch]$NoShortcut,
    [switch]$NoPath
)

$ErrorActionPreference = "Stop"

if (-not $env:LOCALAPPDATA) {
    throw "LOCALAPPDATA is not set. Use -InstallDir to choose an install location."
}

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo = "dikmri/fvCapture"
$assetName = "fvCapture-windows-x86_64.zip"
if ($Version -eq "latest") {
    $downloadUrl = "https://github.com/$repo/releases/latest/download/$assetName"
} else {
    $downloadUrl = "https://github.com/$repo/releases/download/$Version/$assetName"
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("fvCapture-install-" + [Guid]::NewGuid().ToString("N"))
$archivePath = Join-Path $tempRoot $assetName
$extractDir = Join-Path $tempRoot "extract"

function Add-ToUserPath([string]$PathToAdd) {
    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    $segments = @()
    if ($current) {
        $segments = $current -split ";" | Where-Object { $_ }
    }
    $exists = $segments | Where-Object {
        [string]::Equals($_, $PathToAdd, [StringComparison]::OrdinalIgnoreCase)
    }
    if (-not $exists) {
        [Environment]::SetEnvironmentVariable("Path", (($segments + $PathToAdd) -join ";"), "User")
    }
    if (($env:Path -split ";") -notcontains $PathToAdd) {
        $env:Path = "$env:Path;$PathToAdd"
    }
}

try {
    New-Item -ItemType Directory -Force -Path $tempRoot, $extractDir, $InstallDir | Out-Null

    Write-Host "Downloading $assetName..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath -UseBasicParsing

    Write-Host "Extracting to $InstallDir..."
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force
    Copy-Item -Path (Join-Path $extractDir "*") -Destination $InstallDir -Recurse -Force

    $binDir = Join-Path $InstallDir "bin"
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null
    Set-Content -Path (Join-Path $binDir "fvCapture.cmd") -Encoding ASCII -Value "@echo off`r`n""%~dp0..\fvCapture.exe"" %*`r`n"
    Set-Content -Path (Join-Path $binDir "fv-capture.cmd") -Encoding ASCII -Value "@echo off`r`n""%~dp0..\fv-capture.exe"" %*`r`n"

    if (-not $NoPath) {
        Add-ToUserPath $binDir
    }

    if (-not $NoShortcut) {
        $programsDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\fvCapture"
        New-Item -ItemType Directory -Force -Path $programsDir | Out-Null
        $shell = New-Object -ComObject WScript.Shell
        $shortcut = $shell.CreateShortcut((Join-Path $programsDir "fvCapture.lnk"))
        $shortcut.TargetPath = Join-Path $InstallDir "fvCapture.exe"
        $shortcut.WorkingDirectory = $InstallDir
        $shortcut.Save()
    }

    Write-Host "fvCapture installed to $InstallDir"
    if (-not $NoPath) {
        Write-Host "Open a new terminal and run: fvCapture"
    } else {
        Write-Host "Run: $InstallDir\fvCapture.exe"
    }
} finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

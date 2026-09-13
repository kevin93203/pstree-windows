[CmdletBinding()]
param(
    [string]$Version = $env:PSTREE_VERSION,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "pstree\bin"),
    [switch]$NoModifyPath
)

$ErrorActionPreference = "Stop"

$repository = "kevin93203/pstree-windows"
$assetName = "pstree-x86_64-pc-windows-msvc.zip"
$checksumName = "SHA256SUMS"
$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()

if ($architecture -ne "X64") {
    throw "pstree-windows currently supports Windows x64 only; detected $architecture."
}

if ($InstallDir.Contains(";")) {
    throw "InstallDir must not contain a semicolon."
}

$InstallDir = [System.IO.Path]::GetFullPath($InstallDir)

if ([string]::IsNullOrWhiteSpace($Version) -or $Version -eq "latest") {
    $releasePath = "latest/download"
} else {
    $releaseTag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    $releasePath = "download/$releaseTag"
}

$baseUrl = "https://github.com/$repository/releases/$releasePath"
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("pstree-install-" + [guid]::NewGuid())
$archivePath = Join-Path $temporaryRoot $assetName
$checksumPath = Join-Path $temporaryRoot $checksumName
$extractPath = Join-Path $temporaryRoot "extract"
$targetPath = Join-Path $InstallDir "pstree.exe"
$stagedPath = Join-Path $InstallDir "pstree.exe.new"

try {
    New-Item -ItemType Directory -Path $temporaryRoot -Force | Out-Null

    Write-Host "Downloading $assetName..."
    Invoke-WebRequest -Uri "$baseUrl/$assetName" -OutFile $archivePath
    Invoke-WebRequest -Uri "$baseUrl/$checksumName" -OutFile $checksumPath

    $expectedHash = $null
    foreach ($line in Get-Content -LiteralPath $checksumPath) {
        if ($line -match ("^\s*([0-9a-fA-F]{64})\s+\*?" + [regex]::Escape($assetName) + "\s*$")) {
            $expectedHash = $Matches[1]
            break
        }
    }

    if ($null -eq $expectedHash) {
        throw "No checksum found for $assetName."
    }

    $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash
    if ($actualHash -ine $expectedHash) {
        throw "Checksum verification failed for $assetName."
    }

    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractPath -Force
    $sourcePath = Join-Path $extractPath "pstree.exe"
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "The release archive does not contain pstree.exe."
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -LiteralPath $sourcePath -Destination $stagedPath -Force
    try {
        Move-Item -LiteralPath $stagedPath -Destination $targetPath -Force
    } catch {
        Remove-Item -LiteralPath $stagedPath -Force -ErrorAction SilentlyContinue
        throw "Could not replace $targetPath. Close any running pstree process and try again. $($_.Exception.Message)"
    }

    $pathChanged = $false
    if (-not $NoModifyPath) {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $pathEntries = @()
        if (-not [string]::IsNullOrWhiteSpace($userPath)) {
            $pathEntries = @($userPath -split ";" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
        }

        $normalizedInstallDir = $InstallDir.TrimEnd("\")
        $alreadyOnPath = @($pathEntries | Where-Object {
            $_.Trim().TrimEnd("\") -ieq $normalizedInstallDir
        }).Count -gt 0

        if (-not $alreadyOnPath) {
            $newPath = (@($pathEntries) + $InstallDir) -join ";"
            [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            $pathChanged = $true
        }
    }

    $versionOutput = (& $targetPath --version 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "Installed pstree.exe failed its version check."
    }

    Write-Host "Installed $versionOutput to $targetPath"
    if ($pathChanged) {
        Write-Host "Restart your terminal for the PATH change to take effect."
    }
} finally {
    Remove-Item -LiteralPath $stagedPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $temporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}

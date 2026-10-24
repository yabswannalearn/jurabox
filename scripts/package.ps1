[CmdletBinding()]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
Push-Location -LiteralPath $taskRoot
try {
    if (-not $SkipBuild) {
        cargo build --release --locked --timings
        if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    }
    $taskExecutable = Join-Path $taskRoot 'target\release\jurabox.exe'
    if (-not (Test-Path -LiteralPath $taskExecutable)) { throw 'Build the release executable first.' }
    $taskArtifacts = Join-Path $taskRoot 'target\release-artifacts'
    $taskPackage = Join-Path $taskArtifacts 'Jurabox-windows-x64'
    New-Item -ItemType Directory -Force -Path $taskPackage | Out-Null
    Copy-Item -LiteralPath $taskExecutable -Destination (Join-Path $taskPackage 'jurabox.exe')
    Copy-Item -LiteralPath (Join-Path $taskRoot 'scripts\PORTABLE-README.md') -Destination (Join-Path $taskPackage 'README.md')
    Copy-Item -LiteralPath (Join-Path $taskRoot 'scripts\update.ps1') -Destination (Join-Path $taskPackage 'update.ps1')
    $taskArchive = Join-Path $taskArtifacts 'Jurabox-windows-x64.zip'
    Compress-Archive -LiteralPath $taskPackage -DestinationPath $taskArchive -Force
    $taskHash = Get-FileHash -LiteralPath $taskArchive -Algorithm SHA256
    "$($taskHash.Hash) *$([IO.Path]::GetFileName($taskArchive))" | Set-Content -Encoding ascii -LiteralPath "$taskArchive.sha256"
    $taskHash | Format-List
    Write-Output "Portable pilot package: $taskArchive"
} finally { Pop-Location }

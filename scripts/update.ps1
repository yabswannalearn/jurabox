[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$InstallDirectory)
$ErrorActionPreference = 'Stop'
$taskSource = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot 'jurabox.exe')).Path
$taskDestination = [IO.Path]::GetFullPath((Join-Path $InstallDirectory 'jurabox.exe'))
if ($taskSource -eq $taskDestination) { throw 'Extract the new package to a separate folder first.' }
$taskRunning = Get-CimInstance Win32_Process -Filter "Name='jurabox.exe'" |
    Where-Object { $_.ExecutablePath -and ([IO.Path]::GetFullPath($_.ExecutablePath) -eq $taskDestination) }
if ($taskRunning) { throw 'Use Stop services and quit in the old Jurabox first. Its manager, session host, and command keeper must exit before replacing the executable.' }
New-Item -ItemType Directory -Force -Path $InstallDirectory | Out-Null
Copy-Item -LiteralPath $taskSource -Destination $taskDestination
Write-Output "Updated $taskDestination. Saved projects and private session evidence were preserved."

# Install the per-user build silently into a temporary folder, check it, uninstall it, and check
# that only the program's own folder and settings were removed.
# Usage: powershell -ExecutionPolicy Bypass -File tools\test-installer.ps1 dist\Prism-Reference-Manager-0.1.4-setup.exe

#
# It refuses to run where Prism Reference Manager is really installed or has settings. The test copy has the same name, so installing it
# takes over the real copy's entry in Windows' installed apps list, its Start menu shortcut and its prism-rm link, and uninstalling it then
# removes all of those and deletes the real settings: the real copy stays on disk but Windows and the hubs no longer see it.
# Run it in a Windows account that does not have the app (or remove the app first).

param([Parameter(Mandatory = $true)][string]$Setup)
$ErrorActionPreference = "Stop"
$realEntry = Get-ChildItem HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall -ErrorAction SilentlyContinue | Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq "Prism Reference Manager" } | Select-Object -First 1
$realCopy = Join-Path $env:LOCALAPPDATA "Prism Reference Manager\prism-reference-manager.exe"
$realSettings = Join-Path $env:APPDATA "org.prism.referencemanager"
if ($realEntry -or (Test-Path $realCopy) -or (Test-Path $realSettings)) {
    "Prism Reference Manager is installed in this Windows account, or has settings here. This test installs and removes a copy with the same name,"
    "which would take away the real copy's Windows entry, shortcut and link, and delete its settings. Nothing was done."
    "Run it in a Windows account that does not have Prism Reference Manager."
    exit 2
}
$target = Join-Path $env:TEMP ("prm-install-test-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
$settings = Join-Path $env:APPDATA "org.prism.referencemanager"
$webcache = Join-Path $env:LOCALAPPDATA "org.prism.referencemanager"
$neighbor = Join-Path (Split-Path $target) ("prm-neighbor-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force $neighbor | Out-Null
Set-Content (Join-Path $neighbor "keep.txt") "must survive"
$libraryProbe = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "Prism Reference Libraries"
$libraryExisted = Test-Path $libraryProbe
$settingsExisted = Test-Path $settings
$results = @()
function Check($name, $ok) { $script:results += [pscustomobject]@{ Check = $name; Passed = [bool]$ok } }

Start-Process -FilePath $Setup -ArgumentList "/S", "/D=$target" -Wait
Check "Program installed into the chosen folder" (Test-Path (Join-Path $target "prism-reference-manager.exe"))
Check "Uninstaller present" (Test-Path (Join-Path $target "uninstall.exe"))
Check "License and notices installed" ((Test-Path (Join-Path $target "LICENSE.txt")) -and (Test-Path (Join-Path $target "THIRD-PARTY.md")))
$key = Get-ChildItem HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall | Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq "Prism Reference Manager" } | Select-Object -First 1
Check "Per-user uninstall entry registered (no administrator rights)" ($null -ne $key)

# Simulate settings written by the program.
New-Item -ItemType Directory -Force $settings | Out-Null
Set-Content (Join-Path $settings "settings.json") "{}"

Start-Process -FilePath (Join-Path $target "uninstall.exe") -ArgumentList "/S", "_?=$target" -Wait
Start-Sleep -Seconds 2
Check "Program files removed" (-not (Test-Path (Join-Path $target "prism-reference-manager.exe")))
Check "Settings folder removed" (-not (Test-Path $settings))
Check "Web view cache folder removed" (-not (Test-Path $webcache))
Check "Neighbouring folder untouched" (Test-Path (Join-Path $neighbor "keep.txt"))
Check "Library folder in Documents unchanged" ((Test-Path $libraryProbe) -eq $libraryExisted)
$key2 = Get-ChildItem HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall | Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq "Prism Reference Manager" } | Select-Object -First 1
Check "Uninstall entry removed" ($null -eq $key2)
Remove-Item -Recurse -Force $neighbor -ErrorAction SilentlyContinue
if (Test-Path $target) { Remove-Item -Recurse -Force $target -ErrorAction SilentlyContinue }
if ($settingsExisted) { "Note: a settings folder existed before the test and was removed by the uninstaller." }
$results | Format-Table -AutoSize
if ($results.Passed -contains $false) { exit 1 }

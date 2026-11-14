# Build a release: tests, notices and SBOM, the per-user installer, and a portable zip.
# Output: dist\  (installer, portable zip, SHA-256 checksums). Nothing here publishes anything.
#
# -Sign signs the program, the installer's parts, the uninstaller and the setup file with Azure Artifact
# Signing during the build (tools\sign_windows.py; the account names come from OPENSTAGE_SIGN_* environment
# variables and the sign-in from `az login`). Without -Sign the build is unsigned.

# Update files: set TAURI_SIGNING_PRIVATE_KEY (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD) and pass
# -UpdateBaseUrl to also produce the installer's update signature and latest.json. See docs\updates.md.
param([string]$UpdateBaseUrl = "", [switch]$Sign)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "== Interface libraries"
Push-Location app
if (-not (Test-Path node_modules)) { npm install --no-audit --no-fund }
node scripts/vendor.mjs
Pop-Location

Write-Host "== Tests"
cargo test -p prm-core --release
if ($LASTEXITCODE -ne 0) { throw "Tests failed; release stopped." }

Write-Host "== Third-party notices, SBOM and license check"
python tools/third_party.py
if ($LASTEXITCODE -ne 0) { throw "A dependency license needs review; release stopped." }

$signScript = Join-Path $root "tools\sign_windows.py"
if ($Sign) {
  Write-Host "== Signing: checking everything it needs"
  python -I $signScript --ready
  if ($LASTEXITCODE -ne 0) { throw "Signing is not ready (see above); nothing was built." }
}

Write-Host "== Installer"
Push-Location app
$updates = [bool]$env:TAURI_SIGNING_PRIVATE_KEY -and [bool]$UpdateBaseUrl
$bundle = [ordered]@{}
if ($updates) { $bundle.createUpdaterArtifacts = $true }
# Tauri runs the sign command for each file it signs (the program, NSIS plugins, the uninstaller, the setup).
if ($Sign) { $bundle.windows = @{ signCommand = @{ cmd = "python"; args = @("-I", $signScript, "--description", "Prism Reference Manager", "%1") } } }
if ($bundle.Count) {
  $cfg = Join-Path $env:TEMP "prm-build-config.json"
  [IO.File]::WriteAllText($cfg, (@{ bundle = $bundle } | ConvertTo-Json -Depth 6), (New-Object Text.UTF8Encoding $false))
  npx tauri build --config $cfg
} else {
  npx tauri build
}
if ($LASTEXITCODE -ne 0) { throw "Installer build failed." }
Pop-Location

$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force $dist | Out-Null
$setup = Get-ChildItem "target\release\bundle\nsis\*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
$checkScript = Join-Path $root "tools\check_signatures.py"
if ($Sign) {
  # The setup file and every program packed inside it (unpacked with 7-Zip).
  Write-Host "== Signing: checking the installer"
  python -I $checkScript $setup.FullName
  if ($LASTEXITCODE -ne 0) { throw "A file is not signed and timestamped; nothing was copied to dist." }
}
Copy-Item $setup.FullName (Join-Path $dist "Prism-Reference-Manager-$version-setup.exe") -Force

if ($updates) {
  Write-Host "== Update signature and latest.json"
  $setupName = "Prism-Reference-Manager-$version-setup.exe"
  $sig = Get-Content ($setup.FullName + ".sig") -Raw
  Copy-Item ($setup.FullName + ".sig") (Join-Path $dist "$setupName.sig") -Force
  $manifest = [ordered]@{
    version   = $version
    notes     = "See the release notes for this version."
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = @{ "windows-x86_64" = @{ signature = $sig.Trim(); url = ($UpdateBaseUrl.TrimEnd("/") + "/" + $setupName) } }
  }
  [IO.File]::WriteAllText((Join-Path $dist "latest.json"), ($manifest | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))
}

Write-Host "== Browser capture extension"
$extZip = Join-Path $dist "Prism-Reference-Manager-$version-browser-capture.zip"
if (Test-Path $extZip) { Remove-Item $extZip }
Compress-Archive -Path "extension\*" -DestinationPath $extZip

Write-Host "== Portable zip"
# Tauri signs the copy of the program that goes into the installer and then restores the one in
# target\release, so the portable copy is signed here.
if ($Sign) {
  python -I $signScript --description "Prism Reference Manager" "target\release\prism-reference-manager.exe"
  if ($LASTEXITCODE -ne 0) { throw "The portable program could not be signed." }
}
$stage = Join-Path $env:TEMP "prm-portable-$version"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "target\release\prism-reference-manager.exe" $stage
Copy-Item LICENSE, NOTICE, THIRD-PARTY.md $stage
Copy-Item -Recurse extension (Join-Path $stage "extension")
Set-Content (Join-Path $stage "portable.txt") "This file makes Prism Reference Manager keep its settings and default library in the data folder next to the program. Delete it to use your Windows profile instead." -Encoding utf8
$zip = Join-Path $dist "Prism-Reference-Manager-$version-portable.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
if ($Sign) {
  Write-Host "== Signing: checking the portable zip"
  python -I $checkScript --zip $zip
  if ($LASTEXITCODE -ne 0) { throw "The portable zip holds a program that is not signed and timestamped." }
}

Get-ChildItem $dist -File | Where-Object { $_.Extension -ne ".txt" } | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content (Join-Path $dist "SHA256SUMS.txt") -Encoding ascii
Get-ChildItem $dist
if ($Sign) { Write-Host "Done. The program and installer are signed and timestamped." } else { Write-Host "Done. These builds are unsigned." }

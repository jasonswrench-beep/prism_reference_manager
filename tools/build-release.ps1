# Build a release: tests, notices and SBOM, the per-user installer, and a portable zip.
# Output: dist\  (installer, portable zip, SHA-256 checksums). Nothing here publishes anything.
#
# -Sign signs the program, the installer's parts, the uninstaller and the setup file with Azure Artifact
# Signing during the build (tools\sign_windows.py; the account names come from OPENSTAGE_SIGN_* environment
# variables and the sign-in from `az login`). Without -Sign the build is unsigned.

# Update files: when the update-signing key is in the profile folder (.prism-reference-manager-update.key)
# or in TAURI_SIGNING_PRIVATE_KEY, the build also makes the installer's update signature, latest.json and
# previous.json. The key's password comes from TAURI_SIGNING_PRIVATE_KEY_PASSWORD or is asked for.
# -UpdateBaseUrl defaults to this version's download folder on the release page. See docs\updates.md.
param([string]$UpdateBaseUrl = "", [switch]$Sign)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$releasePage = "https://github.com/jasonswrench-beep/prism_reference_manager/releases"

$keyFile = Join-Path $env:USERPROFILE ".prism-reference-manager-update.key"
if (-not $env:TAURI_SIGNING_PRIVATE_KEY -and (Test-Path $keyFile)) {
  $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $keyFile -Raw).Trim()
}
if ($env:TAURI_SIGNING_PRIVATE_KEY) {
  # The public key built into the program must belong to this private key, or no update would verify.
  $pubFile = "$keyFile.pub"
  $built = (Select-String -Path "app\src-tauri\src\updates.rs" -Pattern '^pub const PUBKEY: &str = "(.*)";').Matches[0].Groups[1].Value
  if (-not $built) { throw "The update key is present but PUBKEY in updates.rs is empty; nothing was built." }
  if ((Test-Path $pubFile) -and ((Get-Content $pubFile -Raw).Trim() -ne $built)) {
    throw "PUBKEY in updates.rs does not match $pubFile; nothing was built."
  }
  if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    $sec = Read-Host "Password for the update-signing key (press Enter if it has none)" -AsSecureString
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec))
  }
  if (-not $UpdateBaseUrl) { $UpdateBaseUrl = "$releasePage/download/v$version" }
}

Write-Host "== Interface libraries"
Push-Location app
if (-not (Test-Path node_modules)) { npm install --no-audit --no-fund }
node scripts/vendor.mjs
Pop-Location

# Build paths would otherwise be written into the program (in panic messages and debug data),
# including the builder's Windows user name. Map them to neutral names; later entries win, so the
# specific folders come after the profile as a whole.
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE ".cargo" }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE ".rustup" }
$env:RUSTFLAGS = "--remap-path-prefix=$($env:USERPROFILE)=user --remap-path-prefix=$rustupHome=rustup --remap-path-prefix=$cargoHome=cargo --remap-path-prefix=$root=src"

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

$dist = Join-Path $root "dist"
New-Item -ItemType Directory -Force $dist | Out-Null
$setup = Get-ChildItem "target\release\bundle\nsis\*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1

# The program must not carry the builder's profile folder (and so their user name).
Write-Host "== Checking the program for personal paths"
$bytes = [IO.File]::ReadAllBytes((Join-Path $root "target\release\prism-reference-manager.exe"))
$text = [Text.Encoding]::ASCII.GetString($bytes)
$wide = [Text.Encoding]::Unicode.GetString($bytes)
foreach ($needle in @($env:USERPROFILE, $env:USERPROFILE.Replace("\", "/"), "\Users\$($env:USERNAME)\", "/Users/$($env:USERNAME)/")) {
  if ($text.IndexOf($needle, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or $wide.IndexOf($needle, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
    throw "The program file contains the builder's profile path ($needle); nothing was copied to dist."
  }
}
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

  # The outgoing release's manifest becomes previous.json, which "Return to the previous version" reads.
  $prev = Join-Path $dist "previous.json"
  if (Test-Path $prev) { Remove-Item $prev }
  try {
    $old = Invoke-RestMethod "$releasePage/latest/download/latest.json" -TimeoutSec 30
    if ($old.version -and $old.version -ne $version) {
      [IO.File]::WriteAllText($prev, ($old | ConvertTo-Json -Depth 5), (New-Object Text.UTF8Encoding $false))
      Write-Host "previous.json points at version $($old.version)."
    }
  } catch {
    Write-Host "No published latest.json yet, so no previous.json (normal for the first release with updates)."
  }
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

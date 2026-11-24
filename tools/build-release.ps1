# Build a release: tests, notices and SBOM, the per-user installer, and a portable zip.
# Output: dist\  (installer, portable zip, SHA-256 checksums). Nothing here publishes anything.
#
# It builds only from a clean tree whose commit carries the release tag (v<version>), so the tag always
# names the source the build came from. It never tags or commits.
#
# -Sign signs the program, the installer's parts, the uninstaller and the setup file with Azure Artifact
# Signing during the build (tools\sign_windows.py; the account names come from OPENSTAGE_SIGN_* environment
# variables and the sign-in from `az login`). Without -Sign the build is unsigned.

# Update files: when the update-signing key is in the profile folder (.prism-reference-manager-update.key),
# the build also makes the installer's update signature, latest.json and previous.json. The key is used
# only at the end, on the finished setup (tools\sign-update.ps1), and its password is asked for then.
# -UpdateBaseUrl defaults to this version's download folder on the release page. See docs\updates.md.
param([string]$UpdateBaseUrl = "", [switch]$Sign)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$releasePage = "https://github.com/jasonswrench-beep/prism_reference_manager/releases"

function Assert-Clean([string]$when) {
  $dirty = git status --porcelain
  if ($LASTEXITCODE -ne 0) { throw "git status failed; nothing was built." }
  if ($dirty) { throw "There are uncommitted changes $when, so the build would not match any commit. Commit them, tag that commit, and run again:`n$($dirty -join "`n")" }
}

Assert-Clean "in the source folder"
$head = git rev-parse HEAD
$short = git rev-parse --short=7 HEAD
if (@(git tag --points-at HEAD) -notcontains "v$version") {
  throw "Commit $short is not tagged v$version. Tag the commit first (git tag v$version), then build; nothing was built."
}
Write-Host "Building v$version from commit $short"

# No update-signing secret may be in the environment while dependencies' build scripts run.
foreach ($n in "TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PATH", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD") {
  if (Test-Path "Env:$n") { Remove-Item "Env:$n"; Write-Host "$n was set; it is not used and was removed for this build." }
}

$keyFile = Join-Path $env:USERPROFILE ".prism-reference-manager-update.key"
$updates = Test-Path $keyFile
if ($updates) {
  # The public key built into the program must belong to this private key, or no update would verify.
  $pubFile = "$keyFile.pub"
  $built = (Select-String -Path "app\src-tauri\src\updates.rs" -Pattern '^pub const PUBKEY: &str = "(.*)";').Matches[0].Groups[1].Value
  if (-not $built) { throw "The update key is present but PUBKEY in updates.rs is empty; nothing was built." }
  $conf = (Get-Content "app\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).plugins.updater.pubkey
  if ($conf -ne $built) { throw "The updater pubkey in tauri.conf.json differs from PUBKEY in updates.rs; nothing was built." }
  if ((Test-Path $pubFile) -and ((Get-Content $pubFile -Raw).Trim() -ne $built)) {
    throw "PUBKEY in updates.rs does not match $pubFile; nothing was built."
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
cargo test -p prm-core --release --locked
if ($LASTEXITCODE -ne 0) { throw "Tests failed; release stopped." }

Write-Host "== Third-party notices, SBOM and license check"
python tools/third_party.py
if ($LASTEXITCODE -ne 0) { throw "A dependency license needs review; release stopped." }

$signScript = Join-Path $root "tools\sign_windows.py"
if ($Sign) {
  # A window opened before the signing account was saved to Windows lacks its settings; read them from there.
  foreach ($n in "OPENSTAGE_SIGN_ENDPOINT", "OPENSTAGE_SIGN_ACCOUNT", "OPENSTAGE_SIGN_PROFILE", "OPENSTAGE_SIGN_PUBLISHER") {
    if (-not [Environment]::GetEnvironmentVariable($n, "Process")) {
      $v = [Environment]::GetEnvironmentVariable($n, "User")
      if ($v) { [Environment]::SetEnvironmentVariable($n, $v, "Process") }
    }
  }
  Write-Host "== Signing: checking everything it needs"
  python -I $signScript --ready
  if ($LASTEXITCODE -ne 0) { throw "Signing is not ready (see above); nothing was built." }
}

# The notices are packed into the installer, so remaking them must not have changed anything.
Assert-Clean "after the interface libraries, tests and notices were made"

Write-Host "== Installer"
Push-Location app
$bundle = [ordered]@{}
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
Assert-Clean "after the build"
if ((git rev-parse HEAD) -ne $head) { throw "The commit changed while building; nothing was copied to dist." }

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
# Settings > About must show the commit that was tagged.
if ($text.IndexOf("$short ", [StringComparison]::Ordinal) -lt 0) { throw "The program does not carry the build code $short; nothing was copied to dist." }
$checkScript = Join-Path $root "tools\check_signatures.py"
if ($Sign) {
  # The setup file and every program packed inside it (unpacked with 7-Zip).
  Write-Host "== Signing: checking the installer"
  python -I $checkScript $setup.FullName
  if ($LASTEXITCODE -ne 0) { throw "A file is not signed and timestamped; nothing was copied to dist." }
}
$setupName = "Prism-Reference-Manager-$version-setup.exe"
$distSetup = Join-Path $dist $setupName
Copy-Item $setup.FullName $distSetup -Force
$made = @($setupName)

if ($updates) {
  Write-Host "== Update signature and latest.json"
  & (Join-Path $root "tools\sign-update.ps1") -Setup $distSetup -Version $version -KeyFile $keyFile
  # Checked as an installed copy will check it: the built-in key, and this version in the signed comment.
  cargo run -q -p prm-core --release --locked --example verify_update -- $distSetup "$distSetup.sig" $version
  if ($LASTEXITCODE -ne 0) { Remove-Item "$distSetup.sig"; throw "The update signature is not valid; it was removed." }
  $sig = Get-Content "$distSetup.sig" -Raw
  $made += "$setupName.sig", "latest.json"
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
      $made += "previous.json"
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

# Checksums of this version's files only, to attach to the release.
$made += (Split-Path -Leaf $zip), (Split-Path -Leaf $extZip)
$sums = Join-Path $dist "SHA256SUMS-$version.txt"
$made | Sort-Object | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash (Join-Path $dist $_) -Algorithm SHA256).Hash.ToLower(), $_
} | Set-Content $sums -Encoding ascii
Write-Host "== Files for the release"
Get-Content $sums
Write-Host "Built from commit $short, tagged v$version. The release tag must be this commit."
if ($Sign) { Write-Host "Done. The program and installer are signed and timestamped." } else { Write-Host "Done. These builds are unsigned." }

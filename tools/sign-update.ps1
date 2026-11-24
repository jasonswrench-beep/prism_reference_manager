# Make the update signature (<setup>.sig) for a finished, code-signed setup file.
# build-release.ps1 runs this after everything is built, so the update-signing key and its password are
# never in the environment while npm, cargo or the bundler run. The password is asked for here, reaches
# only the signer, and is cleared afterwards. The signature names the version, which installed copies
# require (requireSignedVersion).
param(
  [Parameter(Mandatory)][string]$Setup,
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$KeyFile,
  [Security.SecureString]$Password
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$Setup = (Resolve-Path $Setup).Path
$KeyFile = (Resolve-Path $KeyFile).Path
$sig = "$Setup.sig"
if (Test-Path $sig) { Remove-Item $sig }
if (-not $Password) { $Password = Read-Host "Password for the update-signing key (press Enter if it has none)" -AsSecureString }

$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Password)
Push-Location (Join-Path $root "app")
try {
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)
  npx tauri signer sign --private-key-path $KeyFile --app-version $Version $Setup | Out-Null
  $code = $LASTEXITCODE
} finally {
  [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr)
  Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
  Pop-Location
}
if ($code -ne 0 -or -not (Test-Path $sig)) { throw "The update signature could not be made (is the password right?)." }

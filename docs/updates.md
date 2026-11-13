# Program updates

The updater is built in and switched off. This version cannot check for updates: the update
feed address and the update-signing public key in `app/src-tauri/src/updates.rs` are empty, and
Settings › About shows "Not set up".

## How it behaves once set up

- Nothing happens until the person clicks **Check now**, or turns on **Check for updates once a
  day** (off by default; administrators can lock it with `"update_check": false` in policy.json).
- A check sends the program's version and Windows architecture to the feed address. It follows
  Settings › Network (the "update" kind can be switched off) and is written to the network log.
- An automatic check only shows a notice. Downloading and installing always needs a click.
- The download's signature is verified against the built-in public key before the installer
  runs. The library is closed cleanly first; the per-user installer keeps settings and libraries.
- Local API clients can never check for or install updates.

## Switching it on (needs the owner)

Update signing uses its own key pair. It is separate from Windows code signing, and both
are needed for a smooth update.

1. Generate the key pair once and keep the private key and its password offline and backed up.
   Losing it means existing installs can no longer accept updates.
   `npx tauri signer generate -w %USERPROFILE%\.prism-reference-manager-update.key`
2. Put the public key in `PUBKEY` and the feed address in `ENDPOINT` in `updates.rs`, for example
   the address where `latest.json` will be published for each release.
3. Build with the private key in the environment and the download folder address:
   `$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content <key file> -Raw` then
   `tools\build-release.ps1 -UpdateBaseUrl <address where the setup files will be published>`
   This adds `<setup>.exe.sig` and `latest.json` to `dist\`.
4. Code-sign the installer during the build (Tauri's Windows signing settings), not afterwards:
   the update signature covers the exact file, so signing it later would break verification.
5. Publish the setup file and `latest.json` at the addresses used above.

The first version with a key and address built in must be installed by hand. Updates work from
that version on.

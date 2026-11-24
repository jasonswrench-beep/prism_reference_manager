# Program updates

The updater is switched on from version 0.1.3. It checks the project's release page:
`app/src-tauri/src/updates.rs` holds the feed address
(`.../releases/latest/download/latest.json`, which skips pre-releases) and the public half of the
update-signing key. Versions 0.1.2 and earlier have no key built in, show "Not set up" in
Settings › About, and must install 0.1.3 by hand.

## How it behaves once set up

- Nothing happens until the person clicks **Check now**, or turns on **Check for updates once a
  day** (off by default; administrators can lock it with `"update_check": false` in policy.json).
- A check sends the program's version and Windows architecture to the feed address. It follows
  Settings › Network (the "update" kind can be switched off) and is written to the network log.
- An automatic check only shows a notice. Downloading and installing always needs a click.
- Installing goes ahead only for the version the person confirmed; if the release page offers
  another one by then, nothing is installed and the person is asked to check again.
- The download's signature is verified against the built-in public key before the installer
  runs. The signature must also name the version it was made for (`requireSignedVersion`), and that
  version must be the one the feed offers. The library is closed cleanly first; the per-user installer keeps settings and libraries.
- Local API clients can never check for or install updates.

## Making a release with updates

Update signing uses its own key pair. It is separate from Windows code signing, and both
are needed for a smooth update.

1. Once: run `Make update key.bat` (kept outside the repository with the other owner tools). It
   runs `npx tauri signer generate -w %USERPROFILE%\.prism-reference-manager-update.key` and asks
   for a password. Keep the secret file and its password offline and backed up. Losing it means
   installed copies can no longer accept updates. The public half (`.key.pub`) goes into `PUBKEY`.
2. Each release: commit everything, including `THIRD-PARTY.md` and `sbom.json` remade by
   `python tools/third_party.py`, and tag that commit `v<version>`. **The release tag must be the
   commit the build came from.** Then run `tools\build-release.ps1 -Sign` (or
   `Build signed release.bat`). The script stops if anything is uncommitted, if the commit does not
   carry that tag, or if the build itself changes a tracked file, and it prints the commit at the
   end. Settings › About shows the same commit.
3. Code signing happens during the build, before the update signature is made: the update
   signature covers the exact signed file, so never sign the installer again afterwards.
4. When the key file is in the profile folder, its public half is checked against `PUBKEY` at the
   start. The secret key is not used until everything is built and code-signed: only then is the
   password asked for, the finished setup signed for this version, and the password cleared. The
   signature is then checked against `PUBKEY`. This adds `<setup>.exe.sig`, `latest.json` and (from
   the second release with updates on) `previous.json` to `dist\`. The download address defaults to
   `.../releases/download/v<version>/`.
5. Attach this version's files listed in `dist\SHA256SUMS-<version>.txt` (the setup file, its
   `.sig`, `latest.json`, `previous.json`, the portable zip and the browser capture zip) and that
   checksum file to the GitHub release, and publish it as a full release (not a pre-release), so
   `latest` points at it.

## Returning to the previous version

`ROLLBACK_ENDPOINT` in `updates.rs` names a second manifest, `previous.json` beside
`latest.json`. The release build makes it by downloading the outgoing release's `latest.json`
(its installer stays published and signed on its own release). Settings › About then offers "Return to the previous
version…": the updater accepts only a version older than the running one, verifies the same
signature, and installs it after a confirmation that explains library formats. A library this
version upgraded opens read-only in the older one until "Return to the format before the last
upgrade" is used (see `library-format.md`).

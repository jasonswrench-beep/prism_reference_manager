//! Check an update signature the way an installed copy will:
//!
//!     cargo run -p prm-core --release --example verify_update -- <setup.exe> <setup.exe.sig> <version>
//!
//! The file must be signed by the key built into the program (PUBKEY in app/src-tauri/src/updates.rs),
//! and the signed comment must name the version, because the updater runs with requireSignedVersion.

use base64::Engine;
use minisign_verify::{PublicKey, Signature};

const UPDATES_RS: &str = include_str!("../../../app/src-tauri/src/updates.rs");

fn text(b64: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn verify(file: &[u8], sig: &str, version: &str) -> Result<(), String> {
    let key = UPDATES_RS.lines().find_map(|l| l.strip_prefix("pub const PUBKEY: &str = \"")?.strip_suffix("\";")).ok_or("PUBKEY not found")?;
    let key = PublicKey::decode(&text(key)?).map_err(|e| format!("public key: {e}"))?;
    let sig = Signature::decode(&text(sig)?).map_err(|e| format!("signature: {e}"))?;
    // Also checks the signature over the comment itself.
    key.verify(file, &sig, false).map_err(|e| format!("not signed by the built-in key: {e}"))?;
    let comment = sig.trusted_comment();
    let signed = comment.split('\t').find_map(|p| p.strip_prefix("version:"));
    if signed != Some(version) {
        return Err(format!("the signed comment names version {signed:?}, not {version} ({comment})"));
    }
    Ok(())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [file, sig, version] = a.as_slice() else {
        eprintln!("usage: verify_update <file> <file.sig> <version>");
        std::process::exit(2);
    };
    let read = |p: &String| std::fs::read(p).map_err(|e| format!("{p}: {e}"));
    let r = read(file).and_then(|f| read(sig).and_then(|s| verify(&f, &String::from_utf8_lossy(&s), version)));
    match r {
        Ok(()) => println!("Update signature OK: built-in key, version {version}."),
        Err(e) => {
            eprintln!("Update signature NOT valid: {e}");
            std::process::exit(1);
        }
    }
}

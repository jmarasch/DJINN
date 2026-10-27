//! Signs releases so DJINN only installs updates made by its maintainers
//! (see docs/releasing.md).
//!
//! - `cargo run -p djinn-platform --example sign -- keygen <secret key file>`
//!   makes a key pair, saves the secret key, and prints the public key for
//!   `djinn-server/src/update.rs`.
//! - `cargo run -p djinn-platform --example sign -- <secret key file> <file>`
//!   writes `<file>.sig`.
//!
//! Keys and signatures are hex. Keep the secret key out of the repository.

use ed25519_dalek::{Signer, SigningKey};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_key(path: &str) -> Result<SigningKey, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    let text = text.trim();
    if text.len() != 64 {
        return Err("the secret key file should hold 64 hex digits".into());
    }
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)?;
    }
    Ok(SigningKey::from_bytes(&bytes))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, path] if command == "keygen" => {
            if std::path::Path::new(path).exists() {
                return Err(format!("{path} exists; not replacing a key").into());
            }
            let mut bytes = [0u8; 32];
            getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
            let key = SigningKey::from_bytes(&bytes);
            std::fs::write(path, hex(&bytes) + "\n")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
            println!("secret key saved to {path}");
            println!("public key: {}", hex(key.verifying_key().as_bytes()));
        }
        [key, file] => {
            let key = read_key(key)?;
            let signature = key.sign(&std::fs::read(file)?);
            std::fs::write(format!("{file}.sig"), hex(&signature.to_bytes()) + "\n")?;
            println!("signed {file}");
        }
        _ => {
            return Err(
                "usage: sign keygen <secret key file> | sign <secret key file> <file>".into(),
            )
        }
    }
    Ok(())
}

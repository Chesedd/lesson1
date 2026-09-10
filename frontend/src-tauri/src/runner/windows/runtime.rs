use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Deserialize)]
struct Inventory {
    python_version: String,
    pandas_version: String,
    python_exe_sha256: String,
    python_dll_sha256: String,
}

pub(crate) fn validate(python: &Path) -> Result<(), String> {
    let root = python.parent().ok_or("invalid controlled runtime path")?;
    let inventory: Inventory = serde_json::from_slice(
        &fs::read(root.join("runtime-inventory.json"))
            .map_err(|_| "controlled runtime inventory is missing")?,
    )
    .map_err(|_| "controlled runtime inventory is invalid")?;
    if inventory.python_version != "3.12.10" || inventory.pandas_version != "2.2.3" {
        return Err("controlled runtime version is invalid".into());
    }
    verify(python, &inventory.python_exe_sha256)?;
    verify(&root.join("python312.dll"), &inventory.python_dll_sha256)?;
    Ok(())
}
fn verify(path: &Path, expected: &str) -> Result<(), String> {
    let digest = format!(
        "{:x}",
        Sha256::digest(fs::read(path).map_err(|_| "controlled runtime file is missing")?)
    );
    if digest.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("controlled runtime integrity check failed".into())
    }
}

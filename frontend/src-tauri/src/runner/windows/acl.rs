use std::{path::Path, process::Command};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::ConvertSidToStringSidW, PSID},
};

/// Grants only the run directory to the profile. The runtime grant is read/execute
/// and is applied by the packaging step, never per-run as a writable copy.
pub(super) fn grant_workspace(path: &Path, sid: PSID) -> Result<(), String> {
    grant(path, sid, "(OI)(CI)(M)")
}

pub(super) fn grant_runtime(path: &Path, sid: PSID) -> Result<(), String> {
    grant(path, sid, "(OI)(CI)(RX)")
}

fn grant(path: &Path, sid: PSID, rights: &str) -> Result<(), String> {
    let mut raw = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let mut len = 0;
    unsafe {
        while *raw.add(len) != 0 {
            len += 1;
        }
    }
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, len) });
    unsafe { LocalFree(raw as *mut _) };
    let status = Command::new("icacls.exe")
        .arg(path)
        .args(["/grant", &format!("*{sid}:{rights}"), "/T", "/C", "/Q"])
        .status()
        .map_err(|e| format!("cannot start ACL tool: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("cannot grant AppContainer ACL".into())
    }
}

use super::handles::OwnedHandle;
use std::{ffi::c_void, ptr};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Security::TOKEN_QUERY,
    System::{
        Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

/// Returns the clean Windows system/user-profile environment for the current
/// process token. The caller owns the returned strings, not the UserEnv block.
#[cfg(test)]
pub(super) fn clean_default_entries() -> Result<Vec<String>, String> {
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(last("OpenProcessToken"));
        }
        let _token = OwnedHandle::new(token)?;

        let mut raw: *mut c_void = ptr::null_mut();
        if CreateEnvironmentBlock(&mut raw, token, 0) == 0 {
            return Err(last("CreateEnvironmentBlock"));
        }
        struct EnvironmentBlock(*mut c_void);
        impl Drop for EnvironmentBlock {
            fn drop(&mut self) {
                unsafe { DestroyEnvironmentBlock(self.0) };
            }
        }
        let block = EnvironmentBlock(raw);
        parse_block(block.0.cast())
    }
}

#[cfg(test)]
unsafe fn parse_block(mut cursor: *const u16) -> Result<Vec<String>, String> {
    if cursor.is_null() {
        return Err("CreateEnvironmentBlock returned a null block".into());
    }
    let mut entries = Vec::new();
    while *cursor != 0 {
        let start = cursor;
        while *cursor != 0 {
            cursor = cursor.add(1);
        }
        let length = cursor.offset_from(start) as usize;
        entries.push(
            String::from_utf16(std::slice::from_raw_parts(start, length))
                .map_err(|error| format!("environment block contains invalid UTF-16: {error}"))?,
        );
        cursor = cursor.add(1);
    }
    Ok(entries)
}

#[cfg(test)]
pub(super) fn entry_name(entry: &str) -> Option<&str> {
    // Windows' hidden per-drive current-directory entries are `=C:=C:\\...`.
    // Their separator is consequently the second equals sign.
    let separator = if entry.starts_with('=') {
        entry[1..].find('=').map(|index| index + 1)?
    } else {
        entry.find('=')?
    };
    Some(&entry[..separator])
}

#[cfg(test)]
fn last(stage: &str) -> String {
    format!("{stage}: {}", std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::entry_name;

    #[test]
    fn parses_normal_and_hidden_environment_entry_names() {
        assert_eq!(entry_name("TEMP=C:\\Temp"), Some("TEMP"));
        assert_eq!(entry_name("=C:=C:\\work"), Some("=C:"));
        assert_eq!(entry_name("missing-separator"), None);
    }
}

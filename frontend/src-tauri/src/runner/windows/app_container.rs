use std::{ffi::c_void, path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::ConvertSidToStringSidW,
        FreeSid,
        Isolation::{
            CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
            GetAppContainerFolderPath,
        },
        PSID,
    },
    System::Com::CoTaskMemFree,
};

const PROFILE_NAME: &str = "ru.lesson1.desktop.student-runner";

pub(super) struct AppContainerIdentity {
    pub sid: PSID,
}

impl AppContainerIdentity {
    pub fn open_or_create() -> Result<Self, String> {
        let name = wide(PROFILE_NAME);
        let display = wide("Python student runner");
        let description = wide("Isolated identity for untrusted lesson code");
        let mut sid: PSID = ptr::null_mut();
        let created = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                display.as_ptr(),
                description.as_ptr(),
                ptr::null(),
                0,
                &mut sid,
            )
        };
        const HRESULT_ALREADY_EXISTS: u32 = 0x8007_00B7;
        if created != 0 && created as u32 != HRESULT_ALREADY_EXISTS {
            return Err(format!(
                "cannot create AppContainer profile: 0x{created:08x}"
            ));
        }
        if created as u32 == HRESULT_ALREADY_EXISTS {
            let derived =
                unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
            if derived != 0 {
                return Err(format!("cannot derive AppContainer SID: 0x{derived:08x}"));
            }
        }
        if sid.is_null() {
            return Err("AppContainer profile returned no SID".into());
        }
        Ok(Self { sid })
    }

    /// Resolves the profile's canonical container-local app-data directory.
    /// This deliberately has no fallback to the caller's profile environment.
    pub fn local_app_data_path(&self) -> Result<PathBuf, String> {
        unsafe {
            let mut sid_string = ptr::null_mut();
            if ConvertSidToStringSidW(self.sid, &mut sid_string) == 0 {
                return Err(last("ConvertSidToStringSidW"));
            }
            if sid_string.is_null() {
                return Err("ConvertSidToStringSidW returned a null string".into());
            }
            struct LocalString(*mut u16);
            impl Drop for LocalString {
                fn drop(&mut self) {
                    unsafe { LocalFree(self.0 as *mut c_void) };
                }
            }
            let sid_string = LocalString(sid_string);

            let mut folder = ptr::null_mut();
            let result = GetAppContainerFolderPath(sid_string.0, &mut folder);
            if result < 0 {
                return Err(format!("GetAppContainerFolderPath failed: 0x{result:08x}"));
            }
            if folder.is_null() {
                return Err("GetAppContainerFolderPath returned a null path".into());
            }
            struct TaskString(*mut u16);
            impl Drop for TaskString {
                fn drop(&mut self) {
                    unsafe { CoTaskMemFree(self.0 as *const c_void) };
                }
            }
            let folder = TaskString(folder);
            let path = decode_nul_terminated(folder.0)?;
            if path.is_empty() {
                return Err("GetAppContainerFolderPath returned an empty path".into());
            }
            Ok(PathBuf::from(path))
        }
    }
}

impl Drop for AppContainerIdentity {
    fn drop(&mut self) {
        unsafe { FreeSid(self.sid as *mut c_void) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

unsafe fn decode_nul_terminated(value: *const u16) -> Result<String, String> {
    if value.is_null() {
        return Err("null UTF-16 string".into());
    }
    let mut length = 0;
    while *value.add(length) != 0 {
        length += 1;
    }
    String::from_utf16(std::slice::from_raw_parts(value, length))
        .map_err(|error| format!("invalid UTF-16 path: {error}"))
}

fn last(stage: &str) -> String {
    format!("{stage}: {}", std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::decode_nul_terminated;

    #[test]
    fn decodes_valid_utf16_and_rejects_invalid_utf16() {
        let valid = ['C' as u16, ':' as u16, 0];
        assert_eq!(
            unsafe { decode_nul_terminated(valid.as_ptr()) }.unwrap(),
            "C:"
        );
        let invalid = [0xD800, 0];
        assert!(unsafe { decode_nul_terminated(invalid.as_ptr()) }.is_err());
    }

    #[test]
    fn rejects_null_utf16_pointer() {
        assert!(unsafe { decode_nul_terminated(std::ptr::null()) }.is_err());
    }
}

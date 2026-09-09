use std::{ffi::c_void, ptr};
use windows_sys::Win32::Security::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName, FreeSid, PSID,
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
}

impl Drop for AppContainerIdentity {
    fn drop(&mut self) {
        unsafe { FreeSid(self.sid as *mut c_void) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};

/// The only owner used for Win32 kernel handles in the sandbox implementation.
pub(super) struct OwnedHandle(pub HANDLE);

impl OwnedHandle {
    pub fn new(handle: HANDLE) -> Result<Self, String> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

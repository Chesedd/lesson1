use super::handles::OwnedHandle;
use std::mem::{size_of, zeroed};
use windows_sys::Win32::{Foundation::HANDLE, System::JobObjects::*};

pub(super) const MEMORY_LIMIT: usize = 512 * 1024 * 1024;
pub(super) const CPU_LIMIT_100NS: i64 = 3 * 10_000_000;

pub(super) struct Job {
    handle: OwnedHandle,
}
impl Job {
    pub fn create() -> Result<Self, String> {
        let handle =
            OwnedHandle::new(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) })?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_JOB_MEMORY
            | JOB_OBJECT_LIMIT_JOB_TIME;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.BasicLimitInformation.PerJobUserTimeLimit = CPU_LIMIT_100NS;
        limits.JobMemoryLimit = MEMORY_LIMIT;
        let ok = unsafe {
            SetInformationJobObject(
                handle.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self { handle })
    }
    pub fn assign(&self, process: HANDLE) -> Result<(), String> {
        if unsafe { AssignProcessToJobObject(self.handle.0, process) } == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    }
    pub fn terminate(&self) {
        unsafe { TerminateJobObject(self.handle.0, 1) };
    }
}

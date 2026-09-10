use super::{app_container::AppContainerIdentity, handles::OwnedHandle, job::Job};
use std::{
    ffi::c_void,
    fs::File,
    io::Read,
    mem::{size_of, zeroed},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle as StdOwnedHandle},
    path::Path,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0},
    Security::{SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES},
    System::{Pipes::CreatePipe, Threading::*},
};

pub(super) struct ProcessOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: u32,
    pub timed_out: bool,
    pub output_limited: bool,
}

pub(super) fn launch(
    exe: &Path,
    workspace: &Path,
    identity: &AppContainerIdentity,
    job: &Job,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    total_limit: usize,
) -> Result<ProcessOutput, String> {
    unsafe {
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: 1,
        };
        let (stdout_read, stdout_write) = pipe(&mut sa)?;
        let (stderr_read, stderr_write) = pipe(&mut sa)?;
        if SetHandleInformation(
            stdout_read.as_raw_handle() as HANDLE,
            HANDLE_FLAG_INHERIT,
            0,
        ) == 0
        {
            return Err(last());
        }
        if SetHandleInformation(
            stderr_read.as_raw_handle() as HANDLE,
            HANDLE_FLAG_INHERIT,
            0,
        ) == 0
        {
            return Err(last());
        }

        let mut attribute_size = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut attribute_size);
        let mut storage = vec![0usize; attribute_size.div_ceil(size_of::<usize>())];
        let attributes = storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(attributes, 2, 0, &mut attribute_size) == 0 {
            return Err(last());
        }
        struct AttrGuard(LPPROC_THREAD_ATTRIBUTE_LIST);
        impl Drop for AttrGuard {
            fn drop(&mut self) {
                unsafe { DeleteProcThreadAttributeList(self.0) }
            }
        }
        let _attributes_guard = AttrGuard(attributes);
        let mut capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: identity.sid,
            Capabilities: ptr::null_mut(),
            CapabilityCount: 0,
            Reserved: 0,
        };
        if UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &mut capabilities as *mut _ as *mut c_void,
            size_of::<SECURITY_CAPABILITIES>(),
            ptr::null_mut(),
            ptr::null_mut(),
        ) == 0
        {
            return Err(last());
        }
        let mut inherited = [stdout_write.0, stderr_write.0];
        if UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited.as_mut_ptr() as *mut c_void,
            size_of_val(&inherited),
            ptr::null_mut(),
            ptr::null_mut(),
        ) == 0
        {
            return Err(last());
        }

        let mut startup: STARTUPINFOEXW = zeroed();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = 0 as HANDLE;
        startup.StartupInfo.hStdOutput = stdout_write.0;
        startup.StartupInfo.hStdError = stderr_write.0;
        startup.lpAttributeList = attributes;
        let mut info: PROCESS_INFORMATION = zeroed();
        let command = format!("\"{}\" -I -B student.py", exe.display());
        let mut command: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
        let cwd: Vec<u16> = workspace.as_os_str().encode_wide().chain(Some(0)).collect();
        let temp = workspace.display();
        let env = format!("PYTHONIOENCODING=utf-8\0PYTHONUTF8=1\0TEMP={temp}\0TMP={temp}\0\0");
        let mut env: Vec<u16> = env.encode_utf16().collect();
        let flags = EXTENDED_STARTUPINFO_PRESENT
            | CREATE_UNICODE_ENVIRONMENT
            | CREATE_SUSPENDED
            | CREATE_NO_WINDOW;
        if CreateProcessW(
            ptr::null(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            flags,
            env.as_mut_ptr() as *const c_void,
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        ) == 0
        {
            return Err(last());
        }
        let process = OwnedHandle::new(info.hProcess)?;
        let thread_handle = OwnedHandle::new(info.hThread)?;
        // The primary thread remains suspended until assignment succeeds: there is
        // no interval in which Python can create a process outside this job.
        job.assign(process.0)?;
        if ResumeThread(thread_handle.0) == u32::MAX {
            return Err(last());
        }
        drop(stdout_write);
        drop(stderr_write);
        let total = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));
        let out = reader(
            stdout_read,
            stdout_limit,
            total_limit,
            total.clone(),
            exceeded.clone(),
        );
        let err = reader(
            stderr_read,
            stderr_limit,
            total_limit,
            total,
            exceeded.clone(),
        );
        let started = Instant::now();
        let timed_out = loop {
            if exceeded.load(Ordering::Relaxed) {
                job.terminate();
                break false;
            }
            if WaitForSingleObject(process.0, 10) == WAIT_OBJECT_0 {
                break false;
            }
            if started.elapsed() >= timeout {
                job.terminate();
                break true;
            }
        };
        WaitForSingleObject(process.0, 5_000);
        let mut exit_code = 1;
        GetExitCodeProcess(process.0, &mut exit_code);
        let stdout = out.join().unwrap_or_default();
        let stderr = err.join().unwrap_or_default();
        let output_limited = exceeded.load(Ordering::Relaxed);
        if output_limited {
            job.terminate();
        }
        Ok(ProcessOutput {
            stdout,
            stderr,
            exit_code,
            timed_out,
            output_limited,
        })
    }
}

unsafe fn pipe(sa: *mut SECURITY_ATTRIBUTES) -> Result<(StdOwnedHandle, OwnedHandle), String> {
    let (mut read, mut write) = (ptr::null_mut(), ptr::null_mut());
    if CreatePipe(&mut read, &mut write, sa, 0) == 0 {
        return Err(last());
    }
    let read = StdOwnedHandle::from_raw_handle(read as _);
    Ok((read, OwnedHandle::new(write)?))
}
fn reader(
    handle: StdOwnedHandle,
    limit: usize,
    total_limit: usize,
    total: Arc<AtomicUsize>,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut file = File::from(handle);
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let Ok(count) = file.read(&mut buffer) else {
                break;
            };
            if count == 0 {
                break;
            }
            let previous = total.fetch_add(count, Ordering::Relaxed);
            let allowed = count
                .min(limit.saturating_sub(bytes.len()))
                .min(total_limit.saturating_sub(previous));
            bytes.extend_from_slice(&buffer[..allowed]);
            if allowed < count || bytes.len() >= limit || previous + count >= total_limit {
                exceeded.store(true, Ordering::Relaxed);
                break;
            }
        }
        bytes
    })
}
fn last() -> String {
    std::io::Error::last_os_error().to_string()
}

use std::os::windows::ffi::OsStrExt;

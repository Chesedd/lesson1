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
    System::{Pipes::CreatePipe, SystemInformation::GetWindowsDirectoryW, Threading::*},
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
    launch_inner(
        exe,
        workspace,
        identity,
        job,
        timeout,
        stdout_limit,
        stderr_limit,
        total_limit,
        EnvironmentSource::Controlled,
    )
}

enum EnvironmentSource {
    Controlled,
    #[cfg(test)]
    WindowsDefault(Vec<String>),
}

#[cfg(test)]
pub(super) enum DiagnosticEnvironment {
    Controlled,
    WindowsDefault(Vec<String>),
}

#[cfg(test)]
pub(super) fn launch_diagnostic(
    exe: &Path,
    workspace: &Path,
    identity: &AppContainerIdentity,
    job: &Job,
    source: DiagnosticEnvironment,
) -> Result<ProcessOutput, String> {
    let source = match source {
        DiagnosticEnvironment::Controlled => EnvironmentSource::Controlled,
        DiagnosticEnvironment::WindowsDefault(entries) => {
            EnvironmentSource::WindowsDefault(entries)
        }
    };
    launch_inner(
        exe,
        workspace,
        identity,
        job,
        Duration::from_secs(4),
        64 * 1024,
        32 * 1024,
        80 * 1024,
        source,
    )
}

fn launch_inner(
    exe: &Path,
    workspace: &Path,
    identity: &AppContainerIdentity,
    job: &Job,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    total_limit: usize,
    environment: EnvironmentSource,
) -> Result<ProcessOutput, String> {
    if !exe.is_absolute() {
        return Err("controlled Python executable path must be absolute".into());
    }
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
            return Err(last("SetHandleInformation(stdout)"));
        }
        if SetHandleInformation(
            stderr_read.as_raw_handle() as HANDLE,
            HANDLE_FLAG_INHERIT,
            0,
        ) == 0
        {
            return Err(last("SetHandleInformation(stderr)"));
        }

        let mut attribute_size = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut attribute_size);
        let mut storage = vec![0usize; attribute_size.div_ceil(size_of::<usize>())];
        let attributes = storage.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        if InitializeProcThreadAttributeList(attributes, 2, 0, &mut attribute_size) == 0 {
            return Err(last("InitializeProcThreadAttributeList"));
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
            return Err(last("UpdateProcThreadAttribute(SECURITY_CAPABILITIES)"));
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
            return Err(last("UpdateProcThreadAttribute(HANDLE_LIST)"));
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
        let application: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        let cwd: Vec<u16> = workspace.as_os_str().encode_wide().chain(Some(0)).collect();
        let workspace = workspace.as_os_str().to_string_lossy().into_owned();
        let overrides = [
            ("PYTHONIOENCODING", "utf-8"),
            ("PYTHONUTF8", "1"),
            ("TEMP", workspace.as_str()),
            ("TMP", workspace.as_str()),
        ];
        let mut env = match environment {
            EnvironmentSource::Controlled => {
                let system_root = windows_directory()?;
                build_environment_block(
                    overrides
                        .into_iter()
                        .chain(std::iter::once(("SystemRoot", system_root.as_str()))),
                )?
            }
            #[cfg(test)]
            EnvironmentSource::WindowsDefault(entries) => {
                let system_root = windows_directory()?;
                build_environment_block_from_entries(
                    entries,
                    overrides
                        .into_iter()
                        .chain(std::iter::once(("SystemRoot", system_root.as_str()))),
                )?
            }
        };
        let flags = EXTENDED_STARTUPINFO_PRESENT
            | CREATE_UNICODE_ENVIRONMENT
            | CREATE_SUSPENDED
            | CREATE_NO_WINDOW;
        if CreateProcessW(
            application.as_ptr(),
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
            return Err(last("CreateProcessW"));
        }
        let process = OwnedHandle::new(info.hProcess)?;
        let thread_handle = OwnedHandle::new(info.hThread)?;
        // The primary thread remains suspended until assignment succeeds: there is
        // no interval in which Python can create a process outside this job.
        job.assign(process.0)
            .map_err(|error| format!("AssignProcessToJobObject: {error}"))?;
        if ResumeThread(thread_handle.0) == u32::MAX {
            return Err(last("ResumeThread"));
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
        return Err(last("CreatePipe"));
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
fn last(stage: &str) -> String {
    format!("{stage}: {}", std::io::Error::last_os_error())
}

fn windows_directory() -> Result<String, String> {
    let mut buffer = vec![0u16; 260];
    loop {
        let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return Err(last("GetWindowsDirectoryW"));
        }
        if (length as usize) < buffer.len() {
            buffer.truncate(length as usize);
            return String::from_utf16(&buffer)
                .map_err(|error| format!("GetWindowsDirectoryW returned invalid UTF-16: {error}"));
        }
        buffer.resize(length as usize, 0);
    }
}

fn build_environment_block<'a>(
    variables: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<u16>, String> {
    let mut variables: Vec<_> = variables.into_iter().collect();
    for (name, value) in &variables {
        if name.contains('\0') || value.contains('\0') {
            return Err("Windows environment names and values must not contain NUL".into());
        }
    }
    variables.sort_by(|left, right| {
        left.0
            .to_uppercase()
            .cmp(&right.0.to_uppercase())
            .then_with(|| left.0.cmp(right.0))
    });

    let mut block = Vec::new();
    for (name, value) in variables {
        block.extend(name.encode_utf16());
        block.push('=' as u16);
        block.extend(value.encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(test)]
fn build_environment_block_from_entries<'a>(
    entries: Vec<String>,
    overrides: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<u16>, String> {
    use super::environment::entry_name;

    let overrides: Vec<_> = overrides.into_iter().collect();
    let mut combined: Vec<String> = entries
        .into_iter()
        .filter(|entry| {
            entry_name(entry).is_some_and(|name| {
                !overrides
                    .iter()
                    .any(|(override_name, _)| name.eq_ignore_ascii_case(override_name))
            })
        })
        .collect();
    combined.extend(
        overrides
            .into_iter()
            .map(|(name, value)| format!("{name}={value}")),
    );
    if combined.iter().any(|entry| entry.contains('\0')) {
        return Err("Windows environment entries must not contain NUL".into());
    }
    combined.sort_by(|left, right| {
        left.to_uppercase()
            .cmp(&right.to_uppercase())
            .then_with(|| left.cmp(right))
    });
    let mut block = Vec::new();
    for entry in combined {
        block.extend(entry.encode_utf16());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::{build_environment_block, build_environment_block_from_entries};

    #[test]
    fn controlled_environment_block_is_sorted_and_double_nul_terminated() {
        let workspace = r"C:\student workspace";
        let block = build_environment_block([
            ("TMP", workspace),
            ("SystemRoot", r"C:\Windows"),
            ("PYTHONUTF8", "1"),
            ("TEMP", workspace),
            ("PYTHONIOENCODING", "utf-8"),
        ])
        .unwrap();
        let entries: Vec<_> = block[..block.len() - 1]
            .split(|unit| *unit == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| String::from_utf16(entry).unwrap())
            .collect();

        assert_eq!(
            entries,
            [
                "PYTHONIOENCODING=utf-8",
                "PYTHONUTF8=1",
                r"SystemRoot=C:\Windows",
                r"TEMP=C:\student workspace",
                r"TMP=C:\student workspace",
            ]
        );
        assert!(!entries.iter().any(|entry| entry.starts_with("PATH=")));
        assert_eq!(&block[block.len() - 2..], &[0, 0]);
    }

    #[test]
    fn default_block_preserves_hidden_entries_and_applies_safe_overrides() {
        let block = build_environment_block_from_entries(
            vec![
                "=C:=C:\\old".into(),
                "Path=C:\\Windows".into(),
                "TEMP=C:\\user-temp".into(),
            ],
            [("TEMP", "C:\\workspace"), ("TMP", "C:\\workspace")],
        )
        .unwrap();
        let entries: Vec<_> = block[..block.len() - 1]
            .split(|unit| *unit == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| String::from_utf16(entry).unwrap())
            .collect();
        assert!(entries.contains(&"=C:=C:\\old".into()));
        assert!(entries.contains(&"Path=C:\\Windows".into()));
        assert!(entries.contains(&"TEMP=C:\\workspace".into()));
        assert!(!entries.contains(&"TEMP=C:\\user-temp".into()));
        assert_eq!(&block[block.len() - 2..], &[0, 0]);
    }
}

use std::os::windows::ffi::OsStrExt;

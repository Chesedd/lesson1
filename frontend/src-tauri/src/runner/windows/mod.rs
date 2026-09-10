mod acl;
mod app_container;
mod environment;
mod handles;
mod job;
mod process;
pub(crate) mod runtime;

use super::SandboxReadiness;
use super::{result, sanitize, RunExerciseResultV1, RunStatus};

pub(super) fn readiness() -> SandboxReadiness {
    match app_container::AppContainerIdentity::open_or_create() {
        Ok(_) => SandboxReadiness::Ready,
        Err(_) => SandboxReadiness::AppcontainerUnavailable,
    }
}
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(super) struct PythonLaunch<'a> {
    pub executable: &'a Path,
    pub workspace: &'a Path,
    pub wall_timeout: Duration,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
    pub total_output_limit: usize,
}

pub(super) fn run_python(config: PythonLaunch<'_>) -> Result<RunExerciseResultV1, String> {
    let started = Instant::now();
    let identity = app_container::AppContainerIdentity::open_or_create()?;
    acl::grant_workspace(config.workspace, identity.sid)?;
    acl::grant_runtime(
        config.executable.parent().ok_or("invalid runtime")?,
        identity.sid,
    )?;
    let job = job::Job::create()?;
    let output = process::launch(
        config.executable,
        config.workspace,
        &identity,
        &job,
        config.wall_timeout,
        config.stdout_limit,
        config.stderr_limit,
        config.total_output_limit,
    )?;
    let stdout = sanitize(&String::from_utf8_lossy(&output.stdout), config.workspace);
    let stderr = sanitize(&String::from_utf8_lossy(&output.stderr), config.workspace);
    let status = if output.timed_out {
        RunStatus::Timeout
    } else if output.output_limited {
        RunStatus::OutputLimit
    } else if output.exit_code == 0 {
        RunStatus::Success
    } else if output.exit_code == 0xC0000017 {
        RunStatus::ResourceLimit
    } else if stderr.contains("SyntaxError:") {
        RunStatus::SyntaxError
    } else if stderr.contains("ImportError:") || stderr.contains("ModuleNotFoundError:") {
        RunStatus::ImportError
    } else {
        RunStatus::RuntimeError
    };
    Ok(result(
        status,
        &stdout,
        &stderr,
        Some(output.exit_code as i32),
        started,
        output.output_limited,
    ))
}

#[cfg(test)]
pub(super) fn environment_diagnostic(executable: &Path) -> Result<(), String> {
    use super::Workspace;
    use std::fs;

    super::validate_runtime(executable)?;
    let workspace = Workspace::create().map_err(|error| format!("create workspace: {error}"))?;
    fs::write(workspace.path.join("student.py"), "print(\"Привет\")")
        .map_err(|error| format!("write payload: {error}"))?;
    let identity = app_container::AppContainerIdentity::open_or_create()?;
    acl::grant_workspace(&workspace.path, identity.sid)?;
    acl::grant_runtime(executable.parent().ok_or("invalid runtime")?, identity.sid)?;

    let defaults = environment::clean_default_entries()?;
    let default_entry_count = defaults.len();
    let mut names: Vec<_> = defaults
        .iter()
        .filter_map(|entry| environment::entry_name(entry).map(str::to_owned))
        .collect();
    names.sort_by_key(|name| name.to_uppercase());

    println!("WINDOWS APPCONTAINER ENVIRONMENT DIAGNOSTIC\n");
    for (label, source) in [
        (
            "controlled_minimal",
            process::DiagnosticEnvironment::Controlled,
        ),
        (
            "windows_default_noninherited",
            process::DiagnosticEnvironment::WindowsDefault(defaults),
        ),
    ] {
        let job = job::Job::create()?;
        println!("{label}:");
        match process::launch_diagnostic(executable, &workspace.path, &identity, &job, source) {
            Ok(output) => {
                println!("  CreateProcessW: SUCCESS");
                println!("  process_exit: {}", output.exit_code);
            }
            Err(error) => println!("  {error}"),
        }
        if label == "windows_default_noninherited" {
            println!("  environment_entries: {default_entry_count}");
            println!("  environment_variable_names: {}", names.join(", "));
        }
    }
    println!("\nPRODUCTION ENVIRONMENT POLICY UNCHANGED");
    Ok(())
}

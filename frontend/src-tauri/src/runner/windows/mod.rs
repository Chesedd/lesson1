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

#[cfg(test)]
pub(super) fn environment_minimization_diagnostic(executable: &Path) -> Result<(), String> {
    use super::Workspace;
    use std::fs;

    #[derive(Debug)]
    enum TrialOutcome {
        CreateFailed(String),
        ExitZero,
        ExitNonzero(u32),
        Timeout,
        OutputLimit,
    }

    impl TrialOutcome {
        fn passed(&self) -> bool {
            matches!(self, Self::ExitZero)
        }

        fn label(&self) -> String {
            match self {
                Self::CreateFailed(error) => format!("CREATE_FAILED ({error})"),
                Self::ExitZero => "PROCESS_STARTED_EXIT_0".into(),
                Self::ExitNonzero(code) => format!("PROCESS_STARTED_EXIT_NONZERO ({code})"),
                Self::Timeout => "TIMEOUT".into(),
                Self::OutputLimit => "OUTPUT_LIMIT".into(),
            }
        }
    }

    fn launch_trial(
        executable: &Path,
        workspace: &Path,
        identity: &app_container::AppContainerIdentity,
        entries: Vec<String>,
    ) -> TrialOutcome {
        let job = match job::Job::create() {
            Ok(job) => job,
            Err(error) => return TrialOutcome::CreateFailed(error),
        };
        match process::launch_diagnostic(
            executable,
            workspace,
            identity,
            &job,
            process::DiagnosticEnvironment::WindowsDefault(entries),
        ) {
            Err(error) => TrialOutcome::CreateFailed(error),
            Ok(output) if output.timed_out => TrialOutcome::Timeout,
            Ok(output) if output.output_limited => TrialOutcome::OutputLimit,
            Ok(output) if output.exit_code == 0 => TrialOutcome::ExitZero,
            Ok(output) => TrialOutcome::ExitNonzero(output.exit_code),
        }
    }

    const FIXED: [&str; 5] = [
        "PYTHONIOENCODING",
        "PYTHONUTF8",
        "SystemRoot",
        "TEMP",
        "TMP",
    ];

    super::validate_runtime(executable)?;
    let workspace = Workspace::create().map_err(|error| format!("create workspace: {error}"))?;
    fs::write(workspace.path.join("student.py"), "print(\"Привет\")")
        .map_err(|error| format!("write payload: {error}"))?;
    let identity = app_container::AppContainerIdentity::open_or_create()?;
    acl::grant_workspace(&workspace.path, identity.sid)?;
    acl::grant_runtime(executable.parent().ok_or("invalid runtime")?, identity.sid)?;

    let defaults = environment::clean_default_entries()?;
    let baseline_count = defaults.len();
    // Fixed values are replaced inside launch_diagnostic. Exclude all baseline
    // copies of them so candidates contain only removable Windows entries.
    let mut candidates = Vec::new();
    for entry in defaults {
        let name = environment::entry_name(&entry)
            .ok_or("CreateEnvironmentBlock returned an entry without a name")?;
        if !FIXED.iter().any(|fixed| name.eq_ignore_ascii_case(fixed)) {
            candidates.push(entry);
        }
    }
    candidates.sort_by(|left, right| {
        let left = environment::entry_name(left).unwrap_or(left);
        let right = environment::entry_name(right).unwrap_or(right);
        left.to_uppercase()
            .cmp(&right.to_uppercase())
            .then_with(|| left.cmp(right))
    });
    let candidate_count = candidates.len();
    let mut launch_attempt_count = 1usize;

    println!("WINDOWS APPCONTAINER ENVIRONMENT MINIMIZATION\n");
    println!("baseline_entries: {baseline_count}");
    println!("fixed:");
    for name in FIXED {
        println!("  {name}");
    }

    let baseline = launch_trial(executable, &workspace.path, &identity, candidates.clone());
    println!("baseline_sanity: {}", baseline.label());
    if !baseline.passed() {
        println!("\nPRODUCTION ENVIRONMENT POLICY UNCHANGED");
        return Err(format!(
            "known-good baseline sanity check failed: {}",
            baseline.label()
        ));
    }

    let mut retained = candidates;
    loop {
        let mut removed_this_pass = 0;
        let mut index = 0;
        while index < retained.len() {
            let name = environment::entry_name(&retained[index])
                .ok_or("CreateEnvironmentBlock returned an entry without a name")?
                .to_owned();
            let mut trial_entries = retained.clone();
            trial_entries.remove(index);
            launch_attempt_count += 1;
            let outcome = launch_trial(
                executable,
                &workspace.path,
                &identity,
                trial_entries.clone(),
            );
            if outcome.passed() {
                println!("trial remove {name}: PASS");
                retained = trial_entries;
                removed_this_pass += 1;
            } else {
                println!("trial remove {name}: REQUIRED ({})", outcome.label());
                index += 1;
            }
        }
        if removed_this_pass == 0 {
            break;
        }
    }

    println!("\nminimal_required_additional:");
    for entry in &retained {
        println!("  {}", environment::entry_name(entry).unwrap());
    }
    let mut final_names: Vec<String> = FIXED.iter().map(|name| (*name).to_owned()).collect();
    final_names.extend(
        retained
            .iter()
            .filter_map(|entry| environment::entry_name(entry).map(str::to_owned)),
    );
    final_names.sort_by_key(|name| name.to_uppercase());
    println!("\nfinal_environment_names:");
    for name in final_names {
        println!("  {name}");
    }

    println!("\nfinal_verification:");
    let mut stable = true;
    for run in 1..=2 {
        launch_attempt_count += 1;
        let outcome = launch_trial(executable, &workspace.path, &identity, retained.clone());
        println!("  run_{run}: {}", outcome.label());
        stable &= outcome.passed();
    }
    if !stable {
        println!("MINIMIZATION RESULT UNSTABLE");
        println!("\nPRODUCTION ENVIRONMENT POLICY UNCHANGED");
        return Err("final environment did not pass twice consecutively".into());
    }
    println!("  CreateProcessW: SUCCESS");
    println!("  process_exit: 0");
    println!("\nbaseline_count: {baseline_count}");
    println!("candidate_count: {candidate_count}");
    println!("removed_count: {}", candidate_count - retained.len());
    println!("required_count: {}", retained.len());
    println!("launch_attempt_count: {launch_attempt_count}");
    println!("\nPRODUCTION ENVIRONMENT POLICY UNCHANGED");
    Ok(())
}

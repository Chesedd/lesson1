use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(not(windows))]
use std::process::{Command, Stdio};

#[cfg(windows)]
mod windows;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxReadiness {
    Ready,
    UnsupportedOs,
    RuntimeMissing,
    RuntimeInvalid,
    AppcontainerUnavailable,
    SandboxInitializationFailed,
}

pub const MAX_CODE_BYTES: usize = 64 * 1024;
const WALL_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_STDOUT_BYTES: usize = 64 * 1024;
const MAX_STDERR_BYTES: usize = 32 * 1024;
const MAX_TOTAL_OUTPUT_BYTES: usize = 80 * 1024;
static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunExerciseRequestV1 {
    pub protocol_version: u8,
    pub lesson_id: String,
    pub exercise_id: String,
    pub code: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Success,
    RuntimeError,
    ImportError,
    SyntaxError,
    Timeout,
    OutputLimit,
    ResourceLimit,
    RunnerError,
}

#[derive(Clone, Debug, Serialize)]
pub struct RunExerciseResultV1 {
    pub protocol_version: u8,
    pub status: RunStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub truncated: bool,
}

pub trait PythonRuntimeResolver: Send + Sync {
    fn resolve(&self) -> std::result::Result<PathBuf, String>;
}

struct ControlledRuntimeResolver {
    bundled_root: PathBuf,
}
impl PythonRuntimeResolver for ControlledRuntimeResolver {
    fn resolve(&self) -> std::result::Result<PathBuf, String> {
        #[cfg(all(not(debug_assertions), not(windows)))]
        return Err("production Run requires the Windows sandbox".into());
        #[cfg(all(not(debug_assertions), windows))]
        if std::env::var("LEARNING_APP_ENABLE_PRODUCTION_RUN").as_deref() != Ok("1") {
            return Err("production Run is disabled".into());
        }
        let bundled = self.bundled_root.join(if cfg!(windows) {
            "python.exe"
        } else {
            "bin/python3"
        });
        if bundled.is_file() {
            return Ok(bundled);
        }
        #[cfg(debug_assertions)]
        if let Some(value) = std::env::var_os("LEARNING_APP_PYTHON") {
            let path = PathBuf::from(value);
            if path.is_file() {
                return Ok(path);
            }
            return Err("development Python runtime is missing".into());
        }
        Err("controlled Python runtime is unavailable".into())
    }
}

pub struct PythonRunner {
    assets_root: PathBuf,
    resolver: Box<dyn PythonRuntimeResolver>,
}
impl PythonRunner {
    pub fn new(assets_root: PathBuf, bundled_root: PathBuf) -> Self {
        Self {
            assets_root,
            resolver: Box::new(ControlledRuntimeResolver { bundled_root }),
        }
    }
    #[cfg(test)]
    fn with_resolver(assets_root: PathBuf, resolver: Box<dyn PythonRuntimeResolver>) -> Self {
        Self {
            assets_root,
            resolver,
        }
    }

    pub fn run(&self, code: &str, assets: &Vec<&str>) -> RunExerciseResultV1 {
        let started = Instant::now();
        match self.run_inner(code, assets) {
            Ok(mut result) => {
                result.duration_ms = started.elapsed().as_millis() as u64;
                result
            }
            Err(message) => result(RunStatus::RunnerError, "", &message, None, started, false),
        }
    }

    pub fn sandbox_readiness(&self) -> SandboxReadiness {
        #[cfg(not(windows))]
        {
            SandboxReadiness::UnsupportedOs
        }
        #[cfg(windows)]
        {
            let Ok(python) = self.resolver.resolve() else {
                return SandboxReadiness::RuntimeMissing;
            };
            if validate_runtime(&python).is_err() {
                return SandboxReadiness::RuntimeInvalid;
            }
            windows::readiness()
        }
    }

    fn run_inner(
        &self,
        code: &str,
        assets: &Vec<&str>,
    ) -> std::result::Result<RunExerciseResultV1, String> {
        let python = self.resolver.resolve()?;
        #[cfg(windows)]
        validate_runtime(&python)?;
        let workspace = Workspace::create().map_err(|_| "cannot create run workspace")?;
        File::create(workspace.path.join("student.py"))
            .and_then(|mut f| f.write_all(code.as_bytes()))
            .map_err(|_| "cannot prepare student code")?;
        for filename in assets {
            if Path::new(filename).components().count() != 1 {
                return Err("invalid exercise asset".into());
            }
            fs::copy(
                self.assets_root.join(filename),
                workspace.path.join(filename),
            )
            .map_err(|_| "exercise asset is unavailable")?;
        }
        #[cfg(windows)]
        return windows::run_python(windows::PythonLaunch {
            executable: &python,
            workspace: &workspace.path,
            wall_timeout: WALL_TIMEOUT,
            stdout_limit: MAX_STDOUT_BYTES,
            stderr_limit: MAX_STDERR_BYTES,
            total_output_limit: MAX_TOTAL_OUTPUT_BYTES,
        });

        #[cfg(not(windows))]
        {
            let mut command = Command::new(python);
            command
                .arg("-I")
                .arg("-B")
                .arg("student.py")
                .current_dir(&workspace.path)
                .env_clear()
                .env("PYTHONIOENCODING", "utf-8")
                .env("PYTHONUTF8", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            let mut child = command
                .spawn()
                .map_err(|_| "cannot start controlled Python runtime")?;
            let stdout = child.stdout.take().ok_or("runner stdout unavailable")?;
            let stderr = child.stderr.take().ok_or("runner stderr unavailable")?;
            let total = Arc::new(AtomicUsize::new(0));
            let exceeded = Arc::new(AtomicBool::new(false));
            let out_thread =
                read_bounded(stdout, MAX_STDOUT_BYTES, total.clone(), exceeded.clone());
            let err_thread = read_bounded(stderr, MAX_STDERR_BYTES, total, exceeded.clone());
            let started = Instant::now();
            let (status, timed_out) = loop {
                if exceeded.load(Ordering::Relaxed) {
                    kill_tree(&mut child);
                    break (None, false);
                }
                if let Some(s) = child
                    .try_wait()
                    .map_err(|_| "cannot monitor Python runtime")?
                {
                    break (Some(s), false);
                }
                if started.elapsed() >= WALL_TIMEOUT {
                    kill_tree(&mut child);
                    break (None, true);
                }
                thread::sleep(Duration::from_millis(10));
            };
            let _ = child.wait();
            let stdout = sanitize(
                &String::from_utf8_lossy(&out_thread.join().unwrap_or_default()),
                &workspace.path,
            );
            let stderr = sanitize(
                &String::from_utf8_lossy(&err_thread.join().unwrap_or_default()),
                &workspace.path,
            );
            let truncated = exceeded.load(Ordering::Relaxed);
            let kind = if timed_out {
                RunStatus::Timeout
            } else if truncated {
                RunStatus::OutputLimit
            } else if status.as_ref().is_some_and(|s| s.success()) {
                RunStatus::Success
            } else if stderr.contains("SyntaxError:") {
                RunStatus::SyntaxError
            } else if stderr.contains("ImportError:") || stderr.contains("ModuleNotFoundError:") {
                RunStatus::ImportError
            } else {
                RunStatus::RuntimeError
            };
            Ok(result(
                kind,
                &stdout,
                &stderr,
                status.and_then(|s| s.code()),
                started,
                truncated,
            ))
        }
    }
}

#[cfg(windows)]
fn validate_runtime(python: &Path) -> Result<(), String> {
    windows::runtime::validate(python)
}

fn read_bounded(
    mut reader: impl Read + Send + 'static,
    own_limit: usize,
    total: Arc<AtomicUsize>,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut result = Vec::new();
        let mut buf = [0; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let old = total.fetch_add(n, Ordering::Relaxed);
                    let allowed = n
                        .min(own_limit.saturating_sub(result.len()))
                        .min(MAX_TOTAL_OUTPUT_BYTES.saturating_sub(old));
                    result.extend_from_slice(&buf[..allowed]);
                    if allowed < n || result.len() >= own_limit || old + n >= MAX_TOTAL_OUTPUT_BYTES
                    {
                        exceeded.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
        }
        result
    })
}
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", &format!("-{}", child.id())])
            .status();
    }
    let _ = child.kill();
}
fn sanitize(text: &str, workspace: &Path) -> String {
    text.replace(&workspace.to_string_lossy().to_string(), ".")
        .replace("./student.py", "student.py")
}
fn result(
    status: RunStatus,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    started: Instant,
    truncated: bool,
) -> RunExerciseResultV1 {
    RunExerciseResultV1 {
        protocol_version: 1,
        status,
        stdout: stdout.into(),
        stderr: stderr.into(),
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        truncated,
    }
}

struct Workspace {
    path: PathBuf,
}
impl Workspace {
    fn create() -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "learning-run-{}-{}-{nonce}",
            std::process::id(),
            RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixed(PathBuf);
    impl PythonRuntimeResolver for Fixed {
        fn resolve(&self) -> Result<PathBuf, String> {
            Ok(self.0.clone())
        }
    }
    fn python() -> Option<PathBuf> {
        std::env::var_os("LEARNING_APP_PYTHON")
            .map(PathBuf::from)
            .or_else(|| {
                ["/usr/bin/python3", "/usr/local/bin/python3"]
                    .iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            })
    }
    fn runner() -> Option<PythonRunner> {
        python().map(|p| {
            PythonRunner::with_resolver(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content/assets"),
                Box::new(Fixed(p)),
            )
        })
    }
    #[test]
    fn success_unicode_syntax_runtime_timeout_and_output() {
        let Some(r) = runner() else { return };
        for (code, status) in [
            ("print('Привет')", RunStatus::Success),
            ("for i in range(2)\n print(i)", RunStatus::SyntaxError),
            ("print(1/0)", RunStatus::RuntimeError),
            ("import package_that_does_not_exist", RunStatus::ImportError),
            ("while True: pass", RunStatus::Timeout),
            ("while True: print('x')", RunStatus::OutputLimit),
        ] {
            let got = r.run(code, &vec![]);
            assert_eq!(got.status, status, "{}", got.stderr);
            assert!(got.stdout.len() <= MAX_STDOUT_BYTES);
        }
    }
    #[test]
    fn allowed_asset_and_environment_isolation_and_cleanup() {
        let Some(r) = runner() else { return };
        std::env::set_var("PYTHONPATH", "/definitely/not/inherited");
        let before: Vec<_> = fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|x| x.ok())
            .filter(|x| x.file_name().to_string_lossy().starts_with("learning-run-"))
            .collect();
        let got=r.run("import os\nprint(open('students.csv',encoding='utf8').readline())\nprint(os.environ.get('PYTHONPATH'))",&vec!["students.csv"]);
        assert_eq!(got.status, RunStatus::Success, "{}", got.stderr);
        assert!(got.stdout.contains("None"));
        let after: Vec<_> = fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter_map(|x| x.ok())
            .filter(|x| x.file_name().to_string_lossy().starts_with("learning-run-"))
            .collect();
        assert_eq!(before.len(), after.len());
    }

    #[cfg(windows)]
    #[test]
    fn windows_appcontainer_python_security_matrix() {
        let Some(r) = std::env::var_os("LEARNING_APP_WINDOWS_TEST_PYTHON").map(|python| {
            PythonRunner::with_resolver(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content/assets"),
                Box::new(Fixed(PathBuf::from(python))),
            )
        }) else {
            eprintln!("not run: LEARNING_APP_WINDOWS_TEST_PYTHON is not provisioned");
            return;
        };
        let outside = std::env::temp_dir().join("outside-workspace-secret.txt");
        fs::write(&outside, "secret").unwrap();
        let progress = std::env::temp_dir().join("progress.sqlite3");
        fs::write(&progress, "trusted-state").unwrap();
        let outside_literal = format!("{:?}", outside.to_string_lossy());
        let progress_literal = format!("{:?}", progress.to_string_lossy());
        let cases = [
            ("print('Привет')".into(), RunStatus::Success),
            ("import statistics; print(statistics.mean([1,2,3]))".into(), RunStatus::Success),
            ("open('temp.txt','w').write('ok')".into(), RunStatus::Success),
            ("import pandas as pd; print(pd.read_csv('students.csv').shape)".into(), RunStatus::Success),
            (format!("open({outside_literal}).read()"), RunStatus::RuntimeError),
            (format!("import pathlib; pathlib.Path({outside_literal}).read_text()"), RunStatus::RuntimeError),
            (format!("import shutil; shutil.copy('student.py',{outside_literal})"), RunStatus::RuntimeError),
            (format!("open({progress_literal}).read()"), RunStatus::RuntimeError),
            (format!("import os; os.remove({progress_literal})"), RunStatus::RuntimeError),
            ("import socket; socket.create_connection(('127.0.0.1',9),.2)".into(), RunStatus::RuntimeError),
            ("import socket; socket.create_connection(('1.1.1.1',53),.2)".into(), RunStatus::RuntimeError),
            ("import subprocess,sys; subprocess.run([sys.executable,'-c','print(1)'],check=True)".into(), RunStatus::RuntimeError),
            ("import os; assert 'LEARNING_APP_TEST_SECRET' not in os.environ".into(), RunStatus::Success),
            ("while True: pass".into(), RunStatus::Timeout),
            ("while True: print('flood')".into(), RunStatus::OutputLimit),
        ];
        for (code, expected) in cases {
            let assets = if code.contains("students.csv") {
                vec!["students.csv"]
            } else {
                vec![]
            };
            let actual = r.run(&code, &assets);
            assert_eq!(actual.status, expected, "code={code}\n{}", actual.stderr);
        }
        assert_eq!(fs::read_to_string(&outside).unwrap(), "secret");
        assert_eq!(fs::read_to_string(&progress).unwrap(), "trusted-state");
        fs::remove_file(outside).unwrap();
        fs::remove_file(progress).unwrap();
    }
}

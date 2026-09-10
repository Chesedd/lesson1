use regex::Regex;
#[cfg(test)]
use rusqlite::params;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::Mutex,
};
use tauri::{Manager, State};
use thiserror::Error;

mod runner;
pub use runner::{
    PythonRuntimeResolver, RunExerciseRequestV1, RunExerciseResultV1, RunStatus, SandboxReadiness,
};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("NOT_FOUND: lesson {0}")]
    NotFound(String),
    #[error("invalid content: {0}")]
    InvalidContent(String),
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("run request rejected: {0}")]
    RunRequest(String),
}
type Result<T> = std::result::Result<T, AppError>;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    id: String,
    title: String,
    age_group: String,
    assets: Vec<Asset>,
    lessons: Vec<Lesson>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    id: String,
    filename: String,
    media_type: String,
    description: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Lesson {
    id: String,
    title: String,
    age_group: String,
    content_version: String,
    blocks: Vec<Block>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    id: String,
    title: String,
    order: u32,
    theory: Theory,
    exercises: Vec<Exercise>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Theory {
    parts: Vec<TheoryPart>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TheoryPart {
    #[serde(rename = "heading")]
    Heading { text: String },
    #[serde(rename = "paragraph")]
    Paragraph { text: String },
    #[serde(rename = "code")]
    Code { language: String, code: String },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exercise {
    id: String,
    title: String,
    difficulty: Difficulty,
    statement: String,
    starter_code: String,
    hints: Vec<String>,
    #[serde(default)]
    public_examples: Vec<PublicExample>,
    #[serde(default)]
    asset_ids: Vec<String>,
    order: u32,
    runtime: Runtime,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Python,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Basic,
    Intermediate,
    Advanced,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicExample {
    input: String,
    output: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct PublicExercise {
    id: String,
    title: String,
    difficulty: Difficulty,
    statement: String,
    starter_code: String,
    hints: Vec<String>,
    public_examples: Vec<PublicExample>,
    assets: Vec<Asset>,
    order: u32,
    runtime: Runtime,
}
#[derive(Clone, Debug, Serialize)]
pub struct PublicBlock {
    id: String,
    title: String,
    order: u32,
    theory: Theory,
    exercises: Vec<PublicExercise>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PublicLesson {
    id: String,
    title: String,
    age_group: String,
    content_version: String,
    blocks: Vec<PublicBlock>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Progress {
    lesson_id: String,
    completed: usize,
    total: usize,
    percent: usize,
    completed_exercise_ids: Vec<String>,
}

pub struct ContentRepository {
    track: Track,
}
impl ContentRepository {
    pub fn from_path(path: &Path) -> Result<Self> {
        Self::from_json(&fs::read_to_string(path)?)
    }
    pub fn from_json(raw: &str) -> Result<Self> {
        let track: Track = serde_json::from_str(raw)?;
        validate(&track)?;
        Ok(Self { track })
    }
    pub fn lesson(&self, id: &str) -> Result<&Lesson> {
        self.track
            .lessons
            .iter()
            .find(|x| x.id == id)
            .ok_or_else(|| AppError::NotFound(id.into()))
    }
    pub fn public_lesson(&self, id: &str) -> Result<PublicLesson> {
        let l = self.lesson(id)?;
        let assets: HashMap<_, _> = self.track.assets.iter().map(|a| (&a.id, a)).collect();
        Ok(PublicLesson {
            id: l.id.clone(),
            title: l.title.clone(),
            age_group: l.age_group.clone(),
            content_version: l.content_version.clone(),
            blocks: l
                .blocks
                .iter()
                .map(|b| PublicBlock {
                    id: b.id.clone(),
                    title: b.title.clone(),
                    order: b.order,
                    theory: b.theory.clone(),
                    exercises: b
                        .exercises
                        .iter()
                        .map(|e| PublicExercise {
                            id: e.id.clone(),
                            title: e.title.clone(),
                            difficulty: e.difficulty.clone(),
                            statement: e.statement.clone(),
                            starter_code: e.starter_code.clone(),
                            hints: e.hints.clone(),
                            public_examples: e.public_examples.clone(),
                            assets: e.asset_ids.iter().map(|id| (*assets[id]).clone()).collect(),
                            order: e.order,
                            runtime: e.runtime.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
    }
}
fn validate(t: &Track) -> Result<()> {
    let stable = Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap();
    let nonempty = |s: &str| !s.trim().is_empty();
    if !stable.is_match(&t.id) || !nonempty(&t.title) || !nonempty(&t.age_group) {
        return bad("invalid track");
    };
    let mut asset_ids = HashSet::new();
    for a in &t.assets {
        if !stable.is_match(&a.id)
            || !asset_ids.insert(&a.id)
            || !Regex::new(r"^[A-Za-z0-9_.-]+$")
                .unwrap()
                .is_match(&a.filename)
            || a.filename.contains("..")
            || a.media_type != "text/csv"
            || !nonempty(&a.description)
        {
            return bad("invalid asset");
        }
    }
    let mut lessons = HashSet::new();
    let mut exercises = HashSet::new();
    for l in &t.lessons {
        if !stable.is_match(&l.id)
            || !lessons.insert(&l.id)
            || !nonempty(&l.title)
            || !nonempty(&l.age_group)
            || !nonempty(&l.content_version)
            || l.blocks.is_empty()
        {
            return bad("invalid lesson");
        };
        let mut prior = 0;
        for b in &l.blocks {
            if !stable.is_match(&b.id)
                || b.order <= prior
                || !nonempty(&b.title)
                || b.theory.parts.is_empty()
                || b.exercises.len() != 3
            {
                return bad("invalid block");
            };
            prior = b.order;
            let mut ep = 0;
            for e in &b.exercises {
                if !stable.is_match(&e.id)
                    || !exercises.insert(&e.id)
                    || e.order <= ep
                    || !nonempty(&e.title)
                    || !nonempty(&e.statement)
                    || e.hints.iter().any(|h| !nonempty(h))
                    || e.asset_ids.iter().any(|id| !asset_ids.contains(id))
                {
                    return bad("invalid exercise");
                };
                ep = e.order
            }
        }
    }
    Ok(())
}
fn bad<T>(s: &str) -> Result<T> {
    Err(AppError::InvalidContent(s.into()))
}

pub struct ProgressRepository {
    connection: Mutex<Connection>,
}
impl ProgressRepository {
    pub fn open(path: &Path) -> Result<Self> {
        let c = Connection::open(path)?;
        Self::initialize(c)
    }
    pub fn memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }
    fn initialize(c: Connection) -> Result<Self> {
        c.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS schema_version(version INTEGER PRIMARY KEY); INSERT OR IGNORE INTO schema_version(version) VALUES(1); CREATE TABLE IF NOT EXISTS exercise_completion(id INTEGER PRIMARY KEY, exercise_id TEXT NOT NULL, content_version TEXT NOT NULL, completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE(exercise_id, content_version));")?;
        Ok(Self {
            connection: Mutex::new(c),
        })
    }
    #[cfg(test)]
    fn record_completion(&self, id: &str, version: &str) -> Result<()> {
        self.connection.lock().unwrap().execute(
            "INSERT OR IGNORE INTO exercise_completion(exercise_id,content_version) VALUES(?1,?2)",
            params![id, version],
        )?;
        Ok(())
    }
    fn completed(&self, ids: &[String], version: &str) -> Result<Vec<String>> {
        let c = self.connection.lock().unwrap();
        let mut stmt =
            c.prepare("SELECT exercise_id FROM exercise_completion WHERE content_version=?1")?;
        let found: HashSet<String> = stmt
            .query_map([version], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(ids
            .iter()
            .filter(|id| found.contains(*id))
            .cloned()
            .collect())
    }
}
pub struct Application {
    content: ContentRepository,
    progress: ProgressRepository,
    runner: runner::PythonRunner,
}
impl Application {
    pub fn new(content: ContentRepository, progress: ProgressRepository) -> Self {
        Self::with_paths(
            content,
            progress,
            Path::new("content/assets"),
            Path::new("runtime"),
        )
    }
    pub fn with_paths(
        content: ContentRepository,
        progress: ProgressRepository,
        assets: &Path,
        runtime: &Path,
    ) -> Self {
        Self {
            content,
            progress,
            runner: runner::PythonRunner::new(assets.into(), runtime.into()),
        }
    }
    pub fn get_lesson(&self, id: &str) -> Result<PublicLesson> {
        self.content.public_lesson(id)
    }
    pub fn get_progress(&self, id: &str) -> Result<Progress> {
        let l = self.content.lesson(id)?;
        let ids: Vec<String> = l
            .blocks
            .iter()
            .flat_map(|b| b.exercises.iter().map(|e| e.id.clone()))
            .collect();
        let done = self.progress.completed(&ids, &l.content_version)?;
        let total = ids.len();
        let completed = done.len();
        Ok(Progress {
            lesson_id: id.into(),
            completed,
            total,
            percent: if total == 0 {
                0
            } else {
                completed * 100 / total
            },
            completed_exercise_ids: done,
        })
    }
    pub fn run_exercise(&self, request: RunExerciseRequestV1) -> Result<RunExerciseResultV1> {
        if request.protocol_version != 1 {
            return Err(AppError::RunRequest("unsupported protocol_version".into()));
        }
        let lesson = self.content.lesson(&request.lesson_id)?;
        let exercise = lesson
            .blocks
            .iter()
            .flat_map(|b| &b.exercises)
            .find(|e| e.id == request.exercise_id)
            .ok_or_else(|| AppError::RunRequest("exercise does not belong to lesson".into()))?;
        if request.code.as_bytes().len() > runner::MAX_CODE_BYTES {
            return Err(AppError::RunRequest("code exceeds 65536 bytes".into()));
        }
        let assets: HashMap<_, _> = self
            .content
            .track
            .assets
            .iter()
            .map(|a| (a.id.as_str(), a.filename.as_str()))
            .collect();
        let filenames = exercise
            .asset_ids
            .iter()
            .map(|id| assets[id.as_str()])
            .collect();
        Ok(self.runner.run(&request.code, &filenames))
    }
}
#[tauri::command]
fn get_lesson(
    lesson_id: String,
    state: State<Application>,
) -> std::result::Result<PublicLesson, String> {
    state.get_lesson(&lesson_id).map_err(|e| e.to_string())
}
#[tauri::command]
fn get_lesson_progress(
    lesson_id: String,
    state: State<Application>,
) -> std::result::Result<Progress, String> {
    state.get_progress(&lesson_id).map_err(|e| e.to_string())
}
#[tauri::command]
async fn run_exercise(
    request: RunExerciseRequestV1,
    state: State<'_, Application>,
) -> std::result::Result<RunExerciseResultV1, String> {
    state.run_exercise(request).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let resource = app.path().resource_dir()?;
            let data = app.path().app_data_dir()?;
            fs::create_dir_all(&data)?;
            let state = Application::with_paths(
                ContentRepository::from_path(&resource.join("content/manifest.json")).map_err(
                    |e| tauri::Error::Setup((Box::new(e) as Box<dyn std::error::Error>).into()),
                )?,
                ProgressRepository::open(&data.join("progress.sqlite3")).map_err(|e| {
                    tauri::Error::Setup((Box::new(e) as Box<dyn std::error::Error>).into())
                })?,
                &resource.join("content/assets"),
                &resource.join("runtime/python-3.12.8"),
            );
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_lesson,
            get_lesson_progress,
            run_exercise
        ])
        .run(tauri::generate_context!())
        .expect("failed to run desktop application")
}

#[cfg(test)]
mod tests {
    use super::*;
    const MANIFEST: &str = include_str!("../../../content/manifest.json");
    fn app() -> Application {
        Application::new(
            ContentRepository::from_json(MANIFEST).unwrap(),
            ProgressRepository::memory().unwrap(),
        )
    }
    #[test]
    fn valid_manifest_and_public_dto() {
        let a = app();
        let json = serde_json::to_string(&a.get_lesson("pandas-intro").unwrap()).unwrap();
        assert!(json.contains("students.csv"));
        assert!(!json.contains("asset_ids"));
        assert!(!json.contains("grader"));
    }
    #[test]
    fn invalid_manifest_and_ids_are_rejected() {
        assert!(ContentRepository::from_json("{}").is_err());
        let raw = MANIFEST.replace("pandas-intro", "Pandas/intro");
        assert!(ContentRepository::from_json(&raw).is_err())
    }
    #[test]
    fn traversal_asset_is_rejected() {
        let raw = MANIFEST.replace("students.csv", "../students.csv");
        assert!(ContentRepository::from_json(&raw).is_err())
    }
    #[test]
    fn unknown_lesson_is_reported() {
        assert!(matches!(
            app().get_lesson("missing"),
            Err(AppError::NotFound(_))
        ))
    }
    #[test]
    fn schema_unique_and_idempotence() {
        let r = ProgressRepository::memory().unwrap();
        r.record_completion("x", "1").unwrap();
        r.record_completion("x", "1").unwrap();
        let count: i64 = r
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM exercise_completion", [], |x| x.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let version: i64 = r
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT max(version) FROM schema_version", [], |x| x.get(0))
            .unwrap();
        assert_eq!(version, 1)
    }
    #[test]
    fn progress_is_manifest_derived_and_versioned() {
        let a = app();
        assert_eq!(
            a.get_progress("pandas-intro").unwrap(),
            Progress {
                lesson_id: "pandas-intro".into(),
                completed: 0,
                total: 3,
                percent: 0,
                completed_exercise_ids: vec![]
            }
        );
        a.progress
            .record_completion("load-students-head", "old")
            .unwrap();
        assert_eq!(a.get_progress("pandas-intro").unwrap().completed, 0);
        a.progress
            .record_completion("load-students-head", "1.0.0")
            .unwrap();
        let p = a.get_progress("pandas-intro").unwrap();
        assert_eq!((p.completed, p.total, p.percent), (1, 3, 33))
    }
    #[test]
    fn total_changes_with_manifest_not_stored_percentage() {
        let mut value: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        let blocks = value["lessons"][0]["blocks"].as_array_mut().unwrap();
        let mut second = blocks[0].clone();
        second["id"] = "second-block".into();
        second["order"] = 2.into();
        for exercise in second["exercises"].as_array_mut().unwrap() {
            let id = exercise["id"].as_str().unwrap().to_owned();
            exercise["id"] = format!("second-{id}").into();
        }
        blocks.push(second);
        let changed = Application::new(
            ContentRepository::from_json(&value.to_string()).unwrap(),
            ProgressRepository::memory().unwrap(),
        );
        assert_eq!(app().get_progress("pandas-intro").unwrap().total, 3);
        assert_eq!(changed.get_progress("pandas-intro").unwrap().total, 6);
    }
    #[test]
    fn run_rejects_unknown_and_mismatched_exercises() {
        let a = app();
        let request = |lesson: &str, exercise: &str| RunExerciseRequestV1 {
            protocol_version: 1,
            lesson_id: lesson.into(),
            exercise_id: exercise.into(),
            code: "print(1)".into(),
        };
        assert!(a
            .run_exercise(request("missing", "load-students-head"))
            .is_err());
        assert!(a.run_exercise(request("pandas-intro", "missing")).is_err());
        assert!(a
            .run_exercise(RunExerciseRequestV1 {
                protocol_version: 2,
                ..request("pandas-intro", "load-students-head")
            })
            .is_err());
        assert!(a
            .run_exercise(RunExerciseRequestV1 {
                code: "x".repeat(runner::MAX_CODE_BYTES + 1),
                ..request("pandas-intro", "load-students-head")
            })
            .is_err());
    }
    #[test]
    fn run_never_mutates_progress() {
        let Some(python) = ["/usr/bin/python3", "/usr/local/bin/python3"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
        else {
            return;
        };
        std::env::set_var("LEARNING_APP_PYTHON", python);
        let a = Application::with_paths(
            ContentRepository::from_json(MANIFEST).unwrap(),
            ProgressRepository::memory().unwrap(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../content/assets")
                .as_path(),
            Path::new("missing-runtime"),
        );
        let before = a.get_progress("pandas-intro").unwrap();
        let result = a
            .run_exercise(RunExerciseRequestV1 {
                protocol_version: 1,
                lesson_id: "pandas-intro".into(),
                exercise_id: "load-students-head".into(),
                code: "print('ok')".into(),
            })
            .unwrap();
        assert_eq!(result.status, RunStatus::Success);
        assert_eq!(before, a.get_progress("pandas-intro").unwrap());
    }
    #[test]
    #[ignore = "development runtime integration; set LEARNING_APP_PYTHON to a Python 3.12.8 environment with pandas 2.2.3"]
    fn development_pandas_smoke() {
        let a = app();
        let result = a
            .run_exercise(RunExerciseRequestV1 {
                protocol_version: 1,
                lesson_id: "pandas-intro".into(),
                exercise_id: "load-students-head".into(),
                code: "import pandas as pd\ndf=pd.read_csv('students.csv')\nprint(df.shape)".into(),
            })
            .unwrap();
        assert_eq!(result.status, RunStatus::Success, "{}", result.stderr);
    }
}

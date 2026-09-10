# ADR 0003: constrained Python Run boundary

## Status

Accepted for development only. **Production student-code execution remains blocked** until the Windows OS-isolation design below is implemented and audited.

## Flow and protocol

React calls `LearningService.runExercise` with only `RunExerciseRequestV1 { protocol_version: 1, lesson_id, exercise_id, code }`. Tauri exposes only `run_exercise`; there is no shell plugin or generic process API. Rust resolves the lesson and its exercise, runtime, content version and asset IDs from the validated manifest. `RunExerciseResultV1` contains `protocol_version`, explicit `status` (`success`, `syntax_error`, `runtime_error`, `import_error`, `timeout`, `output_limit`, `runner_error`), bounded `stdout`/`stderr`, optional `exit_code`, `duration_ms`, and `truncated`.

Run never calls the progress repository. It is execution/feedback, not grading.

## Development constrained runner

Each request gets a new OS child process and a uniquely named directory below the OS temporary directory. Rust writes `student.py`, copies only manifest-approved files from Tauri's resource directory, uses the directory as cwd, drains stdout/stderr concurrently with bounds, kills on four-second wall timeout or output overflow, and removes the directory through an RAII guard. Tracebacks replace the workspace path with `student.py`.

This is a process and resource boundary, **not a sandbox**. On Unix development hosts a new process group is created and killed. Windows currently lacks Job Object containment, a restricted token/AppContainer, filesystem mediation, and network denial. A child could still read/write user-accessible files, use the network, allocate excessive memory, or create processes. Consequently release builds require the explicit `LEARNING_APP_ENABLE_PRODUCTION_RUN=1` capability flag as well as a bundled runtime; installers must not set it before isolation is complete.

Trusted limits are: code 65,536 bytes; wall time 4 seconds; stdout 65,536 bytes; stderr 32,768 bytes; combined output 81,920 bytes. The IPC request cannot change them. `-I -B` and an allowlisted clean environment prevent user site packages, `PYTHONPATH`, active virtualenv state, and bytecode writes. This reduces ambient environment coupling but is not an access-control boundary.

## Runtime and packaging

The pinned target is **CPython 3.12.10** with **pandas 2.2.3** (and dependencies locked by the build pipeline). Development requires an absolute executable file in `LEARNING_APP_PYTHON`; PATH is never searched. This interpreter must be a controlled environment with the pinned packages.

The Windows build pipeline will fetch a checksummed, organization-approved CPython distribution and locked wheels during an online build stage, verify SHA-256 values and `python --version`/package versions, then stage this offline bundle:

```
runtime/python-3.12.10/
  python.exe
  python312.dll
  python312.zip
  Lib/site-packages/pandas/
  runtime-manifest.json   # versions and SHA-256 inventory
```

Tauri resources place it beside `content/`. Rust resolves that fixed resource-relative executable, never PATH and never a frontend-supplied location. CI and installer verification must validate the signed runtime inventory. The installed application never downloads Python or packages and exposes no `pip install` workflow.

## Required Windows security sub-step

Before enabling release Run:

1. launch under a restricted token or AppContainer identity with an explicit capability-free profile;
2. grant filesystem access only to the per-run directory and runtime read-only files, not the user profile;
3. deny network through AppContainer capabilities and Windows Filtering Platform policy;
4. put the process in a Job Object with kill-on-close, active-process limit 1 (or an audited child policy), CPU time and memory limits;
5. assign the process to the job before student instructions can execute (suspended creation, assign, resume);
6. test child-process escape, filesystem denial, network denial, memory containment, and cleanup on application crash.

AST scanning is explicitly not a substitute for these controls.

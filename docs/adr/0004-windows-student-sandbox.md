# ADR 0004: Windows boundary for student Python

**Status:** Proposed, implementation awaiting Windows verification

## Decision

Use a stable ordinary AppContainer profile with zero capabilities, a per-run Job Object, explicit workspace/runtime ACLs, a minimal Unicode environment, and bundled CPython 3.12.8 with pandas 2.2.3. Unsafe Win32 code is confined to `runner/windows`; owned handles close through RAII.

LPAC is not selected yet. It cannot honestly be selected without running the probe, CPython standard-library, and pandas matrix on Windows. Ordinary AppContainer is the conservative initial implementation; after the matrix exists, LPAC will be tested and adopted only if it needs no broad capability or ACL grants.

## Process creation sequence

1. Resolve and hash-check the controlled runtime inventory.
2. Create/reuse the stable student AppContainer and obtain its SID.
3. Grant that SID modify rights on only the fresh workspace and read/execute rights on the runtime.
4. Create and fully configure the Job Object.
5. Build a `STARTUPINFOEX` attribute list containing `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` with no capabilities and an explicit two-handle inheritance list.
6. Call `CreateProcessW` suspended.
7. Assign the process to the Job Object and check success.
8. Resume its primary thread. On any earlier error, RAII closes the process/job and the workspace is removed.

This ordering closes the process-before-job escape window. Job limits are `KILL_ON_JOB_CLOSE`, `ACTIVE_PROCESS=1`, `JOB_MEMORY=512 MiB`, and `JOB_TIME=3 seconds`; wall time is four seconds and output remains separately bounded.

## Runtime packaging and validation

`scripts/prepare-python-runtime.ps1` accepts a build-pipeline-provided archive and expected SHA-256, installs pinned pandas from a trusted offline wheelhouse, verifies versions, and emits hashes for `python.exe` and `python312.dll`. Runtime acquisition is a build concern and never happens at application startup. Production launch validates this inventory before student code runs.

## Consequences and limitations

The application fails closed if the runtime or AppContainer setup is unavailable. Non-Windows release builds cannot enable production Run. Debug builds on other systems retain the explicitly development-only constrained runner.

Windows execution, ACL behavior, network denial, limit notification semantics, pandas native DLL loading, and UI integration remain unverified in the present Linux environment. Memory-limit termination is conservatively exposed as `resource_limit` only for the Windows no-memory status; richer Job completion-port attribution is future work. Production is not ready until real Windows evidence closes those gaps.

# Student code threat model

## Status

The Windows boundary is implemented as a security spike but has **not** been verified on a real Windows 10/11 x64 host. Production Run therefore remains disabled by default and this document does not claim that the Definition of Done has been met.

## Trust boundary

Trusted components are the Tauri process, Rust application layer, signed lesson manifest, SQLite progress database, bundled CPython runtime and a future hidden grader. Student source, its stdout/stderr, every file it creates, and future submissions are untrusted.

Only the child CPython process receives the stable `ru.lesson1.desktop.student-runner` AppContainer identity. The desktop process retains its normal identity. The child has no capability SIDs: in particular it has no Internet, private-network, device, or broad-library capability.

Every run has a fresh workspace. Its ACL grants the AppContainer modify access. The packaged runtime receives read/execute access only. No ACL is added to the workspace parent, `%TEMP%`, application data, lesson source, user profile, or `progress.sqlite3`. `TEMP` and `TMP` point at the workspace.

A new Job Object for every run sets kill-on-close, active-process limit 1, 512 MiB job memory, and three seconds of job user time. A four-second wall timer remains independent. The process is created suspended with `STARTUPINFOEX` and AppContainer security capabilities, assigned to the configured job, and resumed only after assignment succeeds. Only the two output pipe write handles are inherited.

## Required invariants

Untrusted code must not be able to read arbitrary user files, write outside its workspace, read or mutate progress state, use the network, create an escaping process, remove resource limits, or affect the Tauri process. Failure to initialize any boundary is fail-closed; Windows production execution never falls back to `std::process::Command`.

The OS boundary—not Python source inspection, module removal, monkeypatching, or trusted-side path validation—must enforce file, network, and process denial.

## Out of scope

This model does not protect against a local administrator deliberately modifying the installation, Windows-kernel exploitation, physical-machine compromise, malicious trusted content/runtime signing infrastructure, or denial of service outside the configured and tested limits. The adversary is student code, not a hostile machine administrator.

## Verification gate

The Windows CI job currently provides compilation and ordinary test coverage. The runtime/pandas job is deliberately not represented as passing until a pinned wheelhouse and real packaged runtime are provisioned. Before enabling production, the Windows test matrix must demonstrate allowed workspace/runtime operations and deny external files (including SQLite), localhost/LAN/external sockets, child processes, environment secrets, memory/CPU/output floods, and surviving descendants. Repeated-run handle/workspace cleanup and the actual Tauri UI flow must also be measured.

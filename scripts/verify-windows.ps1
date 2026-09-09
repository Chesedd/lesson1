[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$Frontend = Join-Path $RepoRoot 'frontend'
$Manifest = Join-Path $Frontend 'src-tauri/Cargo.toml'
$RuntimeRoot = Join-Path $Frontend 'src-tauri/runtime/windows-x86_64'
$ArtifactRoot = Join-Path $RepoRoot 'artifacts/windows-verification'
New-Item -ItemType Directory -Force -Path $ArtifactRoot | Out-Null
$LogPath = Join-Path $ArtifactRoot ("windows-verification-{0}.log" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))

$Results = [ordered]@{
  'Git state' = 'NOT RUN'; 'Node/npm' = 'NOT RUN'; 'Frontend tests' = 'NOT RUN'
  'Frontend build' = 'NOT RUN'; 'Rust/MSVC toolchain' = 'NOT RUN'; 'Rust compile' = 'NOT RUN'
  'Rust tests' = 'NOT RUN'; 'AppContainer core' = 'NOT RUN'; 'Filesystem isolation' = 'NOT RUN'
  'Network isolation' = 'NOT RUN'; 'Process isolation' = 'NOT RUN'; 'Resource limits' = 'NOT RUN'
  'Controlled Python' = 'NOT RUN'; 'pandas' = 'NOT RUN'; 'Tauri compile/build' = 'NOT RUN'
  'UI smoke' = 'MANUAL REQUIRED'
}
$ExitCode = 0

function Invoke-Native {
  param([Parameter(Mandatory=$true)][string]$File, [string[]]$Arguments = @(), [string]$WorkingDirectory = $RepoRoot)
  Write-Host ("`n> {0} {1}" -f $File, ($Arguments -join ' ')) -ForegroundColor Cyan
  Push-Location $WorkingDirectory
  try {
    & $File @Arguments 2>&1 | ForEach-Object { Write-Host $_ }
    $code = $LASTEXITCODE
  } finally { Pop-Location }
  if ($code -ne 0) { throw "Command failed with exit code ${code}: $File $($Arguments -join ' ')" }
}

function Find-VsDeveloperCommand {
  $vswhereCandidates = @(
    (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'),
    (Join-Path $env:ProgramFiles 'Microsoft Visual Studio/Installer/vswhere.exe')
  )
  $onPath = Get-Command vswhere.exe -ErrorAction SilentlyContinue
  if ($onPath) { $vswhereCandidates = @($onPath.Source) + $vswhereCandidates }
  foreach ($vswhere in ($vswhereCandidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -Unique)) {
    $installation = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1)
    if ($installation) {
      $dev = Join-Path $installation 'Common7/Tools/VsDevCmd.bat'
      if (Test-Path $dev) { return $dev }
    }
  }
  $roots = @(${env:ProgramFiles(x86)}, $env:ProgramFiles) | Where-Object { $_ }
  foreach ($root in $roots) {
    $patterns = @(
      (Join-Path $root 'Microsoft Visual Studio/*/*/Common7/Tools/VsDevCmd.bat'),
      (Join-Path $root 'Microsoft Visual Studio/*/*/VC/Auxiliary/Build/vcvars64.bat')
    )
    foreach ($pattern in $patterns) {
      $found = Get-ChildItem $pattern -File -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
      if ($found) { return $found.FullName }
    }
  }
  return $null
}

function Import-VsEnvironment {
  param([Parameter(Mandatory=$true)][string]$DeveloperCommand)
  Write-Host "Importing MSVC environment into this PowerShell process only: $DeveloperCommand"
  $switches = if ([IO.Path]::GetFileName($DeveloperCommand) -ieq 'VsDevCmd.bat') { '-arch=x64 -host_arch=x64' } else { '' }
  $lines = & $env:ComSpec /d /s /c ('""{0}" {1} >nul && set"' -f $DeveloperCommand, $switches) 2>&1
  if ($LASTEXITCODE -ne 0) { throw "Visual Studio developer environment initialization failed: $($lines -join [Environment]::NewLine)" }
  foreach ($line in $lines) {
    $position = $line.IndexOf('=')
    if ($position -gt 0) { [Environment]::SetEnvironmentVariable($line.Substring(0, $position), $line.Substring($position + 1), 'Process') }
  }
}

function Show-Summary {
  Write-Host "`n================ WINDOWS VERIFICATION SUMMARY ================" -ForegroundColor White
  foreach ($entry in $Results.GetEnumerator()) { Write-Host ("{0,-29} {1}" -f $entry.Key, $entry.Value) }
  if ($Results['Rust compile'] -eq 'FAIL') { Write-Host "`nWINDOWS RUST COMPILE: FAILED" -ForegroundColor Red }
  Write-Host "`nUI SMOKE: MANUAL REQUIRED"
  Write-Host "Command: cd frontend; npm run tauri dev"
  Write-Host @'
Manual checklist:
  1. Start the desktop app.                 6. Verify stdout.
  2. Open the first lesson.                 7. Verify progress did not change.
  3. Open exercise 1.                       8. Run a syntax error.
  4. Change the starter code.               9. Run an infinite loop.
  5. Click Run.                            10. Close and reopen the app.
'@
  Write-Host "Diagnostic log: $LogPath"
  Write-Host 'This harness does not declare production readiness; the complete Windows security matrix is still required.'
}

Start-Transcript -Path $LogPath -Force | Out-Null
try {
  Write-Host "Repository: $RepoRoot"
  Write-Host "PowerShell: $($PSVersionTable.PSVersion) ($($PSVersionTable.PSEdition))"
  if (-not $IsWindows -and $PSVersionTable.PSEdition -eq 'Core') { throw 'This verification harness must run on Windows.' }

  $missing = @('git','node','npm','rustup','rustc','cargo') | Where-Object { -not (Get-Command $_ -ErrorAction SilentlyContinue) }
  if ($missing) {
    if ($missing -contains 'git') { $Results['Git state']='FAIL' }
    if ($missing -contains 'node' -or $missing -contains 'npm') { $Results['Node/npm']='FAIL' }
    if ($missing | Where-Object { $_ -in @('rustup','rustc','cargo') }) { $Results['Rust/MSVC toolchain']='FAIL' }
    throw "Missing prerequisite(s): $($missing -join ', '). Install them before verification; this script does not install software."
  }

  Invoke-Native git @('status','--short','--branch')
  Invoke-Native git @('rev-parse','HEAD')
  Invoke-Native git @('log','-5','--oneline')
  $dirty = & git -C $RepoRoot status --porcelain
  $Results['Git state'] = if ($dirty) { 'WARN' } else { 'PASS' }

  Invoke-Native node @('--version'); Invoke-Native npm @('--version')
  $Results['Node/npm'] = 'PASS'
  Invoke-Native rustup @('--version'); Invoke-Native rustc @('--version','--verbose'); Invoke-Native cargo @('--version')
  $hostLine = (& rustc -vV | Select-String '^host:').Line
  if ($hostLine -notmatch 'host:\s+.+-pc-windows-msvc$') { throw "Rust host must be a Windows MSVC host; found '$hostLine'. Select stable-x86_64-pc-windows-msvc (or the matching MSVC architecture)." }
  $active = (& rustup show active-toolchain 2>&1)
  Write-Host "Active toolchain: $active"
  if ($active -notmatch 'pc-windows-msvc') { throw "The active Rust toolchain is not MSVC: $active" }
  if (-not (Get-Command link.exe -ErrorAction SilentlyContinue)) {
    $developerCommand = Find-VsDeveloperCommand
    if (-not $developerCommand) { throw 'MSVC C++ Build Tools were not found. Install Desktop development with C++, or run this command from an x64 Native Tools Command Prompt for Visual Studio.' }
    Import-VsEnvironment $developerCommand
  }
  $link = Get-Command link.exe -ErrorAction SilentlyContinue
  if (-not $link) { throw 'Build Tools were found but link.exe is still unavailable. Run verification from an x64 Native Tools Command Prompt for Visual Studio.' }
  Write-Host "MSVC linker: $($link.Source)"
  Write-Host '> link.exe /? (diagnostic only; LINK may use a non-zero help exit code)'
  & link.exe /? 2>&1 | ForEach-Object { Write-Host $_ }
  $Results['Rust/MSVC toolchain'] = 'PASS'

  Invoke-Native npm @('ci') $Frontend
  try { Invoke-Native npm @('test','--','--run') $Frontend; $Results['Frontend tests']='PASS' } catch { $Results['Frontend tests']='FAIL'; throw }
  try { Invoke-Native npm @('run','build') $Frontend; $Results['Frontend build']='PASS' } catch { $Results['Frontend build']='FAIL'; throw }

  try { Invoke-Native cargo @('check','--manifest-path',$Manifest,'--all-targets'); $Results['Rust compile']='PASS' }
  catch { $Results['Rust compile']='FAIL'; throw }
  try { Invoke-Native cargo @('test','--manifest-path',$Manifest,'--all-targets','--','--nocapture'); $Results['Rust tests']='PASS' }
  catch { $Results['Rust tests']='FAIL'; throw }

  Write-Host "`n================ CORE SANDBOX ================"
  Write-Host 'The repository currently exposes its Windows security matrix through windows_appcontainer_python_security_matrix.'
  $runtime = Join-Path $RuntimeRoot 'python.exe'
  $inventory = Join-Path $RuntimeRoot 'runtime-inventory.json'
  if (-not (Test-Path $runtime) -or -not (Test-Path $inventory)) {
    $Results['Controlled Python']='BLOCKED'; $Results['pandas']='BLOCKED'
    Write-Warning "PRODUCTION PYTHON RUNTIME - BLOCKED: controlled runtime unavailable at $RuntimeRoot"
    Write-Warning 'The current probe binary is a probe payload, but no independent host-side Rust test entry point launches it yet. Core security categories remain NOT RUN rather than being reported as PASS.'
  } else {
    $env:LEARNING_APP_WINDOWS_TEST_PYTHON = $runtime
    try {
      Invoke-Native cargo @('test','--manifest-path',$Manifest,'windows_appcontainer_python_security_matrix','--','--nocapture')
      $Results['AppContainer core']='PASS'; $Results['Filesystem isolation']='PASS'
      $Results['Network isolation']='PASS'; $Results['Process isolation']='PASS'
      $Results['Controlled Python']='PASS'; $Results['pandas']='PASS'
      Write-Warning 'Resource limits remain NOT RUN: the existing matrix covers wall timeout/output flood but not the full memory/CPU/active-process/descendant matrix.'
    } catch {
      $Results['AppContainer core']='FAIL'; $Results['Filesystem isolation']='FAIL'
      $Results['Network isolation']='FAIL'; $Results['Process isolation']='FAIL'
      $Results['Controlled Python']='FAIL'; $Results['pandas']='FAIL'; throw
    } finally { Remove-Item Env:LEARNING_APP_WINDOWS_TEST_PYTHON -ErrorAction SilentlyContinue }
  }

  try { Invoke-Native npm @('run','tauri','build','--','--no-bundle') $Frontend; $Results['Tauri compile/build']='PASS' }
  catch { $Results['Tauri compile/build']='FAIL'; throw }
} catch {
  $ExitCode = 1
  Write-Host "`nVERIFICATION FAILED: $($_.Exception.Message)" -ForegroundColor Red
} finally {
  Show-Summary
  Stop-Transcript | Out-Null
}
exit $ExitCode

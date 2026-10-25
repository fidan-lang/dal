param(
  [Parameter(Mandatory)][string]$Root,
  [string]$Python = $(if ($IsWindows) { "python" } else { "python3" }),
  [switch]$SkipContainers,
  [switch]$Llvm,
  [string]$FidanHome = "",
  [string]$CodeExecutable = ""
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false
$Root = (Resolve-Path -LiteralPath $Root).Path
$evidence = Join-Path $Root "dal/target/acceptance"
New-Item -ItemType Directory -Force $evidence | Out-Null
$receipts = [Collections.Generic.List[object]]::new()
$npm = if ($IsWindows) { "npm.cmd" } else { "npm" }
$compiler = Join-Path $Root $(if ($IsWindows) { "fidan/target/debug/fidan.exe" } else { "fidan/target/debug/fidan" })
function Get-RustTestCounts([string]$Log) {
  $counts = [ordered]@{ suites = 0; passed = 0; failed = 0; ignored = 0 }
  foreach ($line in Get-Content -LiteralPath $Log) {
    $plain = $line -replace '\x1b\[[0-9;]*m', ''
    if ($plain -match '^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;') {
      $counts.suites++
      $counts.passed += [int]$Matches[1]
      $counts.failed += [int]$Matches[2]
      $counts.ignored += [int]$Matches[3]
    }
  }
  return $counts
}
function Invoke-Gate([string]$Name, [string]$Directory, [string]$Program, [string[]]$Arguments) {
  $log = Join-Path $evidence "$Name.private.log"
  $start = [DateTime]::UtcNow.ToString("o")
  Push-Location (Join-Path $Root $Directory)
  try { & $Program @Arguments *> $log; $code = $LASTEXITCODE }
  catch { $code = 1; "Command launch failed; inspect executable availability" | Add-Content $log }
  finally { Pop-Location }
  if ($null -eq $code) { $code = 0 }
  $receipts.Add([ordered]@{ name=$Name; cwd=$Directory; program=$Program; arguments=$Arguments; started_utc=$start; ended_utc=[DateTime]::UtcNow.ToString("o"); exit_code=$code; rust_test_counts=(Get-RustTestCounts $log); log_sha256=(Get-FileHash $log -Algorithm SHA256).Hash })
  Write-Host "$Name exit=$code"
  if ($code -ne 0) {
    if ($Program -eq 'cargo' -and $Directory -in @('fidan', 'dal')) {
      node (Join-Path $Root 'dal/scripts/summarize-test-failure.mjs') $log (Join-Path $Root $Directory)
    }
    throw "$Name failed. Raw logs stay private; published failure context is limited to known source identities."
  }
}
try {
  foreach ($repo in @("fidan", "dal", "fidan-actions", "fidan-editors", "fidan.dev", "args")) {
    if (-not (Test-Path (Join-Path $Root "$repo/.git"))) { throw "Missing Git checkout: $repo" }
    $receipts.Add([ordered]@{ repository=$repo; sha=(& git -C (Join-Path $Root $repo) rev-parse HEAD) })
  }
  foreach ($repo in @("fidan", "dal")) {
    Invoke-Gate "$repo-fmt" $repo cargo @("fmt", "--all", "--", "--check")
    Invoke-Gate "$repo-build" $repo cargo @("build", "--workspace", "--locked")
    Invoke-Gate "$repo-clippy" $repo cargo @("clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings")
  }
  Invoke-Gate "compiler-tests" fidan cargo @("test", "--workspace", "--locked")
  $env:FIDAN_BIN = $compiler
  $env:FIDAN_ARGS_ROOT = Join-Path $Root "args"
  $env:FIDAN_SETUP_ROOT = Join-Path $Root "fidan-actions"
  $env:FIDAN_EXPECTED_VERSION = (Get-Content (Join-Path $Root "fidan.dev/ci/compiler-candidate.json") -Raw | ConvertFrom-Json).compiler_version
  if ($FidanHome) { $env:FIDAN_HOME = (Resolve-Path $FidanHome).Path }
  else { $env:FIDAN_HOME = Join-Path $Root "fidan/target/acceptance-home" }
  Invoke-Gate "licenses" dal $Python @("scripts/check-license.py", "../fidan", ".", "../fidan-actions", "../fidan-editors", "../fidan.dev", "../args")
  Invoke-Gate "staging-guards" dal $Python @("scripts/test-staging-smoke.py")
  Invoke-Gate "packaging-paths" fidan pwsh @("-NoProfile", "-File", "test/scripts/test-cargo-artifacts.ps1")
  Invoke-Gate "packaging-lto" fidan pwsh @("-NoProfile", "-File", "test/scripts/test-toolchain-build-env.ps1")
  if ($IsWindows) { Invoke-Gate "packaging-windows" fidan pwsh @("-NoProfile", "-File", "test/scripts/test-release-windows.ps1") }
  Invoke-Gate "release-preparation" fidan $Python @("test/scripts/test-release-candidate.py")
  Invoke-Gate "setup-security" fidan-actions node @("--test", "setup/test/setup.test.mjs")
  foreach ($web in @("fidan-editors/vscode", "fidan.dev", "dal/frontend")) {
    $label = $web.Replace("/", "-")
    Invoke-Gate "$label-ci" $web $npm @("ci")
    Invoke-Gate "$label-lint" $web $npm @("run", "lint")
    $script = if ($web -eq "fidan-editors/vscode") { "compile" } else { "build" }
    Invoke-Gate "$label-build" $web $npm @("run", $script)
  }
  Invoke-Gate "editor-cli" fidan-editors/vscode $npm @("run", "check:compatibility")
  Invoke-Gate "site-content" fidan.dev $npm @("run", "check:content")
  Invoke-Gate "site-release-markdown" fidan.dev $npm @("run", "check:release-markdown")
  Invoke-Gate "site-docs" fidan.dev $npm @("run", "docs:check:fidan")
  Invoke-Gate "site-browser" fidan.dev $npm @("run", "e2e")
  Invoke-Gate "registry-markdown" dal/frontend $npm @("run", "check:markdown")
  Invoke-Gate "cranelift-sweep" fidan pwsh @("-NoProfile", "-File", "test/scripts/test_examples_aot.ps1", "-Backend", "cranelift")
  $argsCheck = @("tests/check.mjs")
  if ($Llvm) {
    if (-not $FidanHome) { throw "LLVM validation requires an explicitly installed candidate helper home" }
    $argsCheck += "--llvm"
    Invoke-Gate "llvm-sweep" fidan pwsh @("-NoProfile", "-File", "test/scripts/test_examples_aot.ps1", "-Backend", "llvm", "-Lto", "full", "-FidanHome", $env:FIDAN_HOME)
    Invoke-Gate "llvm-imports" fidan cargo @("test", "--locked", "-p", "fidan-cli", "--test", "dal_registry", "--", "--ignored")
  } else { $receipts.Add(@{name="llvm";status="NOT_EXECUTED_NO_CANDIDATE_HELPER"}) }
  Invoke-Gate "args" args node $argsCheck
  if (-not $SkipContainers) { Invoke-Gate "registry-lifecycle" dal node @("scripts/test-local.mjs", "--e2e") }
  else {
    Invoke-Gate "registry-unit" dal cargo @("test", "--workspace", "--all-targets", "--locked", "--exclude", "dal-server")
    $receipts.Add(@{name="registry-lifecycle";status="NOT_EXECUTED_CONTAINERS_DISABLED"})
  }
  if ($CodeExecutable) {
    $env:VSCODE_EXECUTABLE = (Resolve-Path $CodeExecutable).Path
    Invoke-Gate "extension-host" fidan-editors/vscode node @("scripts/check-extension-host.cjs")
  } else { $receipts.Add(@{name="extension-host";status="NOT_EXECUTED_NO_CODE_EXECUTABLE"}) }
  $vsix = Join-Path $Root "fidan-editors/target/acceptance.vsix"
  New-Item -ItemType Directory -Force (Split-Path $vsix) | Out-Null
  Invoke-Gate "vsix" fidan-editors/vscode $npm @("exec", "--package=@vscode/vsce", "--", "vsce", "package", "--out", $vsix)
} finally {
  # Metadata only: raw Rust/browser logs may contain ephemeral test credentials.
  $receipts | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $evidence "receipts.json") -Encoding utf8
}

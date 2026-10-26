$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'verify-ecosystem.ps1'), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw $errors }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-RustTestCounts' }, $false)
. ([scriptblock]::Create($function.Extent.Text))
$scratch = Join-Path $repo ('target/receipt-regression-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $scratch | Out-Null
$scratch = (Resolve-Path -LiteralPath $scratch).Path
if ([IO.Path]::GetDirectoryName($scratch) -ne (Resolve-Path -LiteralPath (Join-Path $repo 'target')).Path) { throw 'Fixture escaped target' }
$log = Join-Path $scratch 'private.log'
try {
  @(
    'test result: ok. 5 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.01s',
    "$([char]27)[31mtest result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s$([char]27)[0m",
    'token=synthetic-secret; test result: ok. 999 passed; 0 failed; 0 ignored;',
    'https://user:password@example.test?token=synthetic-secret',
    '-----BEGIN PRIVATE KEY-----'
  ) | Set-Content -LiteralPath $log -Encoding utf8
  $counts = Get-RustTestCounts $log
  if ($counts.suites -ne 2 -or $counts.passed -ne 8 -or $counts.failed -ne 1 -or $counts.ignored -ne 2) { throw 'Incorrect aggregate test counts' }
  if (($counts | ConvertTo-Json) -match 'synthetic-secret|password|PRIVATE KEY') { throw 'Raw log content entered the public receipt' }

  # Execute the real environment-selection block without launching any tools.
  $source = $ast.Extent.Text
  $homeSelection = $source.IndexOf('  if ($FidanHome)')
  $compilerTests = $source.IndexOf('  Invoke-Gate "compiler-tests"')
  if ($homeSelection -lt 0 -or $homeSelection -gt $compilerTests) {
    throw 'Candidate helper home must be selected before LLVM-aware workspace tests'
  }
  $start = $source.IndexOf('  $env:FIDAN_BIN = $compiler')
  $end = $source.IndexOf('  if ($FidanHome)', $start)
  $setup = [scriptblock]::Create($source.Substring($start, $end - $start))
  $Root = Join-Path $scratch 'candidate'
  $compiler = Join-Path $Root 'fidan/target/debug/fidan'
  New-Item -ItemType Directory -Force (Join-Path $Root 'args'), (Join-Path $Root 'fidan.dev/ci') | Out-Null
  '{"compiler_version":"9.8.7-test"}' | Set-Content (Join-Path $Root 'fidan.dev/ci/compiler-candidate.json')
  $names = @('FIDAN_BIN', 'FIDAN_ARGS_ROOT', 'FIDAN_SETUP_ROOT', 'FIDAN_EXPECTED_VERSION')
  $previous = @{}
  foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name) }
  try {
    . $setup
    if ($env:FIDAN_BIN -ne $compiler -or $env:FIDAN_ARGS_ROOT -ne (Join-Path $Root 'args') -or
        $env:FIDAN_SETUP_ROOT -ne (Join-Path $Root 'fidan-actions') -or $env:FIDAN_EXPECTED_VERSION -ne '9.8.7-test') {
      throw 'Acceptance must explicitly select the candidate compiler, args, Setup and version'
    }
  } finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name]) }
  }
} finally {
  Remove-Item -LiteralPath $scratch -Recurse -Force
}
Write-Host 'Receipt counts/redaction regression passed.'

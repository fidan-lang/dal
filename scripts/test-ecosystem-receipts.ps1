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
} finally {
  Remove-Item -LiteralPath $scratch -Recurse -Force
}
Write-Host 'Receipt counts/redaction regression passed.'

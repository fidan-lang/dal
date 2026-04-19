$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$templatesRoot = Join-Path $repoRoot ".github"
$templatesRoot = Join-Path $templatesRoot "ISSUE_TEMPLATE"

$templates = @{
  "bug_report.md"      = @{
    Label    = "bug"
    Sections = @(
      "## Describe the bug",
      "## Affected area",
      "## Reproduction steps",
      "## Expected behaviour",
      "## Actual behaviour",
      "## Environment",
      "## Logs",
      "## Additional context"
    )
  }
  "feature_request.md" = @{
    Label    = "enhancement"
    Sections = @(
      "## Summary",
      "## Motivation",
      "## Proposed design",
      "## Example usage",
      "## Compatibility and impact",
      "## Alternatives considered",
      "## Additional context"
    )
  }
}

$requiredFrontMatterKeys = @("name", "about", "title", "labels", "assignees")

function Fail($Message) {
  throw "Issue template validation failed: $Message"
}

foreach ($templateName in $templates.Keys) {
  $templatePath = Join-Path $templatesRoot $templateName

  if (-not (Test-Path -LiteralPath $templatePath -PathType Leaf)) {
    Fail "missing $templateName"
  }

  $lines = Get-Content -LiteralPath $templatePath

  if ($lines.Count -lt 4 -or $lines[0] -ne "---") {
    Fail "$templateName must start with YAML front matter"
  }

  $frontMatterEnd = -1
  for ($i = 1; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -eq "---") {
      $frontMatterEnd = $i
      break
    }
  }

  if ($frontMatterEnd -lt 2) {
    Fail "$templateName has incomplete YAML front matter"
  }

  $frontMatter = @{}
  foreach ($line in $lines[1..($frontMatterEnd - 1)]) {
    if ($line -match "^([A-Za-z0-9_-]+):\s*(.+)$") {
      $frontMatter[$matches[1]] = $matches[2].Trim()
    }
  }

  foreach ($key in $requiredFrontMatterKeys) {
    if (-not $frontMatter.ContainsKey($key) -or [string]::IsNullOrWhiteSpace($frontMatter[$key])) {
      Fail "$templateName is missing front matter key '$key'"
    }
  }

  $expectedLabel = $templates[$templateName].Label
  if ($frontMatter["labels"] -ne $expectedLabel) {
    Fail "$templateName labels must be '$expectedLabel'"
  }

  if ($frontMatter["assignees"] -ne "AppSolves") {
    Fail "$templateName assignees must be 'AppSolves'"
  }

  $body = ($lines[($frontMatterEnd + 1)..($lines.Count - 1)] -join "`n")
  foreach ($section in $templates[$templateName].Sections) {
    if (-not $body.Contains($section)) {
      Fail "$templateName is missing section '$section'"
    }
  }
}

Write-Host "Issue templates are valid."

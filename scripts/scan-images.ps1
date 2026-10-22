param(
  [Parameter(Mandatory)][string]$ServerImage,
  [Parameter(Mandatory)][string]$WorkerImage,
  [Parameter(Mandatory)][string]$Syft,
  [Parameter(Mandatory)][string]$Grype,
  [string]$Output = "target/image-security"
)
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false
New-Item -ItemType Directory -Force $Output | Out-Null
$Output = (Resolve-Path $Output).Path
$records = @()
foreach ($item in @(@('server', $ServerImage), @('worker', $WorkerImage))) {
  $name, $image = $item
  $identity = docker image inspect $image --format '{{.Id}}'
  if ($LASTEXITCODE -ne 0) { throw 'Local candidate image missing' }
  $archive = Join-Path $Output "$name.image.tar"
  docker image save $image -o $archive
  if ($LASTEXITCODE -ne 0) { throw 'Image export failed' }
  & $Syft scan "docker-archive:$archive" -o "syft-json=$(Join-Path $Output "$name.sbom.json")" -o "spdx-json=$(Join-Path $Output "$name.spdx.json")"
  if ($LASTEXITCODE -ne 0) { throw 'SBOM failed' }
  & $Grype "sbom:$(Join-Path $Output "$name.sbom.json")" -o json --file (Join-Path $Output "$name.cves.json")
  if ($LASTEXITCODE -ne 0) { throw 'Vulnerability scan failed' }
  $records += @{ name=$name; image=$image; config_id=$identity; scanned_utc=[DateTime]::UtcNow.ToString('o') }
}
& $Grype db status -o json | Set-Content (Join-Path $Output 'database.json') -Encoding utf8
if ($LASTEXITCODE -ne 0) { throw 'Scanner database status failed' }
$records | ConvertTo-Json | Set-Content (Join-Path $Output 'identities.json') -Encoding utf8
Write-Output 'All raw findings retained. Do not publish image tarballs or treat a completed scan as risk acceptance.'

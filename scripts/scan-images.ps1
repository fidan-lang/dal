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
  docker image save $identity -o $archive
  if ($LASTEXITCODE -ne 0) { throw 'Image export failed' }
  & $Syft scan "docker-archive:$archive" -o "syft-json=$(Join-Path $Output "$name.sbom.json")" -o "spdx-json=$(Join-Path $Output "$name.spdx.json")"
  if ($LASTEXITCODE -ne 0) { throw 'SBOM failed' }
  & $Grype "sbom:$(Join-Path $Output "$name.sbom.json")" -o json --file (Join-Path $Output "$name.cves.json")
  if ($LASTEXITCODE -ne 0) { throw 'Vulnerability scan failed' }
  $metadata = (Get-Content (Join-Path $Output "$name.sbom.json") -Raw | ConvertFrom-Json).source.metadata
  $actual = (docker image inspect $identity | ConvertFrom-Json)[0]
  if ($LASTEXITCODE -ne 0) { throw 'Exact image inspection failed' }
  $configBytes = [Convert]::FromBase64String($metadata.config)
  $sha = [Security.Cryptography.SHA256]::Create()
  try { $configId = 'sha256:' + ([BitConverter]::ToString($sha.ComputeHash($configBytes))).Replace('-', '').ToLowerInvariant() }
  finally { $sha.Dispose() }
  if ($configId -ne $metadata.imageID) { throw 'SBOM image config digest mismatch' }
  $config = [Text.Encoding]::UTF8.GetString($configBytes) | ConvertFrom-Json
  if (($config.rootfs.diff_ids -join ',') -ne ($actual.RootFS.Layers -join ',')) { throw 'SBOM/image root filesystem mismatch' }
  if ($config.config.Labels.'org.opencontainers.image.revision' -ne $actual.Config.Labels.'org.opencontainers.image.revision') { throw 'SBOM source revision mismatch' }
  $records += @{ name=$name; image=$image; image_id=$identity; config_id=$configId;
    source_revision=$actual.Config.Labels.'org.opencontainers.image.revision';
    archive_sha256=(Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant();
    scanned_utc=[DateTime]::UtcNow.ToString('o') }
}
& $Grype db status -o json | Set-Content (Join-Path $Output 'database.json') -Encoding utf8
if ($LASTEXITCODE -ne 0) { throw 'Scanner database status failed' }
$records | ConvertTo-Json | Set-Content (Join-Path $Output 'identities.json') -Encoding utf8
Write-Output 'All raw findings retained. Do not publish image tarballs or treat a completed scan as risk acceptance.'

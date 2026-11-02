#Requires -Version 7.5
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$CaDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$ResourcesFile,
    [switch]$DownloadHelpers
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Run certificate issuance on the Windows administrator workstation; never on the VPS.' }
$config = Get-Content (Join-Path $PSScriptRoot '../deploy/roles-anywhere.json') -Raw | ConvertFrom-Json
$caPath = [IO.Path]::GetFullPath($CaDirectory).TrimEnd('\', '/')
$outputPath = [IO.Path]::GetFullPath($OutputDirectory).TrimEnd('\', '/')
if ($caPath -eq $outputPath -or $caPath.StartsWith($outputPath + '\', [StringComparison]::OrdinalIgnoreCase) -or $outputPath.StartsWith($caPath + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'CA storage and the VPS bundle must be separate, non-nested directories.'
}
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')).TrimEnd('\', '/')
foreach ($privatePath in @($caPath, $outputPath)) {
    if ($privatePath.StartsWith($repository + '\', [StringComparison]::OrdinalIgnoreCase)) {
        # Probe a child file: Git 2.56 can report an unignored trailing-slash path
        # as matching an empty ignore pattern. Directory-only checks are unsafe.
        $relative = [IO.Path]::GetRelativePath($repository, $privatePath).Replace('\', '/') + '/dal-private-directory-check'
        git -C $repository check-ignore --quiet -- $relative
        if ($LASTEXITCODE -ne 0) { throw 'Private material inside the checkout must be under a Git-ignored directory.' }
    }
}
function New-PrivateDirectory([string]$Path) {
    $owner = [Security.Principal.WindowsIdentity]::GetCurrent().User
    $system = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
    if (Test-Path -LiteralPath $Path) {
        $existing = [IO.FileSystemAclExtensions]::GetAccessControl([IO.DirectoryInfo]::new($Path), [Security.AccessControl.AccessControlSections]::Access)
        $rules = $existing.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])
        if (-not $existing.AreAccessRulesProtected -or @($rules | Where-Object { $_.IdentityReference -notin @($owner, $system) -or $_.AccessControlType -ne 'Allow' }).Count) {
            throw 'Existing output directory is not a dedicated private store; choose a fresh directory.'
        }
        return
    }
    [IO.Directory]::CreateDirectory($Path) | Out-Null
    $acl = [IO.FileSystemAclExtensions]::GetAccessControl([IO.DirectoryInfo]::new($Path), [Security.AccessControl.AccessControlSections]::Access)
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($rule in @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))) { $acl.RemoveAccessRuleSpecific($rule) }
    $inherit = [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'
    foreach ($sid in @($owner, $system)) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', $inherit, 'None', 'Allow'))
    }
    # Set only the DACL; writing an unrelated SACL would require SeSecurityPrivilege.
    [IO.FileSystemAclExtensions]::SetAccessControl([IO.DirectoryInfo]::new($Path), $acl)
}
function Write-Utf8([string]$Path, [string]$Value) {
    [IO.File]::WriteAllText($Path, $Value, [Text.UTF8Encoding]::new($false))
}
New-PrivateDirectory $caPath
New-PrivateDirectory $outputPath
$caFile = Join-Path $caPath 'issuer.pfx'
$passwordFile = Join-Path $caPath 'issuer-password.dpapi'
$now = [DateTimeOffset]::UtcNow
Add-Type -AssemblyName System.Security.Cryptography.ProtectedData
if ((Test-Path $caFile) -ne (Test-Path $passwordFile)) { throw 'Incomplete CA store; recover the original pair instead of replacing it.' }
if (-not (Test-Path $caFile)) {
    $rsa = [Security.Cryptography.RSA]::Create(3072)
    try {
        $request = [Security.Cryptography.X509Certificates.CertificateRequest]::new("CN=$($config.issuer_cn), O=Fidan", $rsa, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
        $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($true, $true, 0, $true))
        $usage = [Security.Cryptography.X509Certificates.X509KeyUsageFlags]'KeyCertSign,CrlSign'
        $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new($usage, $true))
        $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509SubjectKeyIdentifierExtension]::new($request.PublicKey, $false))
        $issuer = $request.CreateSelfSigned($now.AddMinutes(-5), $now.AddYears($config.ca_years))
        try {
            $passwordBytes = [Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
            $password = [Convert]::ToBase64String($passwordBytes)
            [IO.File]::WriteAllBytes($passwordFile, [Security.Cryptography.ProtectedData]::Protect([Text.Encoding]::UTF8.GetBytes($password), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser))
            [IO.File]::WriteAllBytes($caFile, $issuer.Export([Security.Cryptography.X509Certificates.X509ContentType]::Pfx, $password))
            Write-Utf8 (Join-Path $caPath 'issuer.pem') ($issuer.ExportCertificatePem() + "`n")
        } finally { $issuer.Dispose(); $password = $null; [Array]::Clear($passwordBytes) }
    } finally { $rsa.Dispose() }
}
$password = [Text.Encoding]::UTF8.GetString([Security.Cryptography.ProtectedData]::Unprotect([IO.File]::ReadAllBytes($passwordFile), $null, [Security.Cryptography.DataProtectionScope]::CurrentUser))
$issuer = [Security.Cryptography.X509Certificates.X509CertificateLoader]::LoadPkcs12([IO.File]::ReadAllBytes($caFile), $password, [Security.Cryptography.X509Certificates.X509KeyStorageFlags]'EphemeralKeySet,Exportable')
$password = $null
try {
    if ($issuer.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -ne $config.issuer_cn -or $issuer.NotAfter.ToUniversalTime() -lt $now.AddDays($config.certificate_days).UtcDateTime) { throw 'CA identity or remaining validity does not match this configuration.' }
    foreach ($entry in $config.workloads.psobject.Properties) {
        $folder = Join-Path $outputPath $entry.Name
        New-PrivateDirectory $folder
        $certificate = Join-Path $folder 'certificate.pem'
        $privateKey = Join-Path $folder 'private-key.pem'
        if ((Test-Path $certificate) -ne (Test-Path $privateKey)) { throw 'Incomplete workload credentials; use a fresh output directory.' }
        if (-not (Test-Path $certificate)) {
            $rsa = [Security.Cryptography.RSA]::Create(3072)
            try {
                $request = [Security.Cryptography.X509Certificates.CertificateRequest]::new("CN=$($entry.Value.cn), OU=$($config.subject_ou), O=Fidan", $rsa, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
                $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($false, $false, 0, $true))
                $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new([Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature, $true))
                $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509SubjectKeyIdentifierExtension]::new($request.PublicKey, $false))
                $leaf = $request.Create($issuer, $now.AddMinutes(-5), $now.AddDays($config.certificate_days), [Security.Cryptography.RandomNumberGenerator]::GetBytes(16))
                try {
                    Write-Utf8 $certificate ($leaf.ExportCertificatePem() + "`n")
                    Write-Utf8 $privateKey ($rsa.ExportPkcs8PrivateKeyPem() + "`n")
                } finally { $leaf.Dispose() }
            } finally { $rsa.Dispose() }
        }
    }
} finally { $issuer.Dispose() }
if ($ResourcesFile) {
    $resources = Get-Content -LiteralPath $ResourcesFile -Raw | ConvertFrom-Json
    $prefix = "arn:aws:rolesanywhere:$($config.region):$($config.account):"
    if ($resources.account -ne $config.account -or $resources.region -ne $config.region -or $resources.trust_anchor_arn -notmatch ('^' + [regex]::Escape($prefix) + 'trust-anchor/[0-9a-f-]{36}$')) { throw 'Unexpected AWS resource identity.' }
    if ($resources.profiles.api -eq $resources.profiles.worker) { throw 'API and worker profiles must be distinct.' }
    foreach ($entry in $config.workloads.psobject.Properties) {
        $profileArn = $resources.profiles.($entry.Name)
        if ($profileArn -notmatch ('^' + [regex]::Escape($prefix) + 'profile/[0-9a-f-]{36}$')) { throw 'Unexpected profile identity.' }
        $roleArn = "arn:aws:iam::$($config.account):role/$($entry.Value.role)"
        $process = '/opt/aws-rolesanywhere/aws_signing_helper credential-process --certificate /run/dal-aws/certificate.pem --private-key /run/dal-aws/private-key.pem'
        $process += " --trust-anchor-arn $($resources.trust_anchor_arn) --profile-arn $profileArn --role-arn $roleArn --session-duration $($config.session_seconds)"
        $text = "[profile $($entry.Value.profile)]`nregion = $($config.region)`ncredential_process = $process`n"
        $file = Join-Path $outputPath "$($entry.Name)/config"
        if ((Test-Path $file) -and [IO.File]::ReadAllText($file) -ne $text) { throw 'Refusing to overwrite an existing different AWS configuration.' }
        Write-Utf8 $file $text
    }
}
if ($DownloadHelpers) {
    New-PrivateDirectory (Join-Path $outputPath 'helper')
    foreach ($entry in $config.helper.psobject.Properties | Where-Object Name -ne 'version') {
        $filename = if ($entry.Name -eq 'linux_amd64') { 'aws_signing_helper' } else { 'aws_signing_helper.exe' }
        $destination = Join-Path $outputPath "helper/$filename"
        if (-not (Test-Path $destination)) {
            $download = "$destination.download"
            Invoke-WebRequest -Uri $entry.Value.url -OutFile $download
            if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value.sha256) { Remove-Item -LiteralPath $download; throw 'Official helper checksum mismatch.' }
            Move-Item -LiteralPath $download -Destination $destination
        }
        if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value.sha256) { throw 'Existing helper checksum mismatch.' }
    }
}
Write-Output 'Prepared local workload credentials. CA material is separate; no AWS resource was changed.'

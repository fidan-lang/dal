# External VPS AWS credentials

Use the existing Cognito pool, FIFO queue and task roles. Roles Anywhere adds
certificate-bound access to those roles; it does not replace ECS trust or enable
Cognito MFA. Settings and official helper hashes live in `roles-anywhere.json`.
No AWS access keys, certificates, private keys or generated credential output belong
in Git, images, build contexts or CI artifacts.

## Prepare credentials on the administrator workstation

Run PowerShell 7.5+ on Windows, outside the VPS. Keep the CA directory separate
from the deployment bundle and preferably on owner-controlled offline storage:

```powershell
./scripts/prepare-vps-aws.ps1 -CaDirectory /private/offline-dal-ca `
  -OutputDirectory ./target/vps-aws-bundle -DownloadHelpers
```

This creates a 5-year signing CA in an encrypted PFX with a Windows-user-bound
DPAPI-protected random password, and separate 90-day API/worker keys and certificates.
It performs no AWS resource changes. Only the public CA certificate is registered
as the AWS trust anchor. Never transfer the CA signing store to the VPS or mount it
in Docker. Arrange an independently recoverable, encrypted offline CA backup before
production; copying a DPAPI blob alone does not make it portable to another user/PC.
Certificate issuance is an offline signing operation; physically air-gapped storage
and backup custody remain administrator responsibilities.

After the approved AWS setup, provide a local, non-secret resource receipt:

```json
{
  "account": "418892985224",
  "region": "eu-central-1",
  "trust_anchor_arn": "arn:aws:rolesanywhere:eu-central-1:418892985224:trust-anchor/<actual-id>",
  "profiles": {
    "api": "arn:aws:rolesanywhere:eu-central-1:418892985224:profile/<actual-api-id>",
    "worker": "arn:aws:rolesanywhere:eu-central-1:418892985224:profile/<actual-worker-id>"
  }
}
```

Pass `-ResourcesFile ./target/vps-aws-resources.json` to the preparation script
using the same directories. It writes separate Linux `credential_process` configs;
rerunning preserves existing keys and refuses a different existing configuration.
The CA store is encrypted and workload directories have owner/SYSTEM-only Windows
ACLs. The public AWS trust policy must match the exact anchor ARN, account, issuer
CN, subject CN and OU; profiles map only the needed CN/OU attributes and each
permits its corresponding existing task role. Credential sessions last 15 minutes.

## VPS installation

On the AMD64 Debian host, transfer only the verified Linux helper and each
workload's three files (`config`, `certificate.pem`, `private-key.pem`). Check the
helper SHA256 against `roles-anywhere.json`; set its owner to root and mode 0755.
Use root-owned parent directories; the two credential directories/files must be
readable by container UID 10001 (directories 0700, files 0400 owned by 10001).
Both images already run non-root; mount each credential directory only in its own
container. Do not mount the common parent, administrator AWS config or CA store.

Set `DAL_AWS_HELPER`, `DAL_API_AWS_DIRECTORY`, `DAL_WORKER_AWS_DIRECTORY` to absolute
host paths. Preserve the existing protected API/worker env files, restricted
PostgreSQL roles and immutable approved image digests. Remove static AWS keys,
web-identity settings, container credential URI settings and custom AWS endpoints
from runtime env files; R2's separately scoped `DAL_STORAGE_*` settings are unchanged.
The override explicitly blanks static AWS credential variables so they cannot take
precedence over the selected profile. It disables EC2 metadata and shared credential
files, and adds only read-only mounts; existing filesystem, privilege and resource
restrictions remain in force.

```sh
docker compose -f deploy/compose.production.yaml -f deploy/compose.roles-anywhere.yaml config --quiet
# Start only after the separately approved DB/provider/deployment gates.
```

Automatic renewal of AWS sessions is provided by the existing AWS SDK for Rust
provider chain and identity cache. Certificates themselves are not automatically
renewed. Before the 90-day expiry, issue new keys/certificates from the offline CA
into a fresh output directory with the same workload identities. Verify them, then
rotate/restart one service at a time; do not replace a live key/certificate pair
piecemeal. A helper, key, certificate or network failure must not cause fallback to
administrator/static credentials. The worker retains its existing retry behavior;
API AWS-dependent operations fail until credentials are available again.

For an exposed key, the operator can disable the affected Roles Anywhere profile
and replace the leaf certificate/key. Already issued sessions can remain valid for
up to 15 minutes. Individual certificate revocation requires an approved CRL
procedure; merely issuing a replacement certificate does not revoke the old one.
No CRL resource or CA service is provisioned automatically by these scripts.

## Verification

```powershell
python scripts/test-vps-aws-policy.py
cargo test -p dal-server --test test_aws_credential_process --locked
cargo build -p dal-server --example aws_credentials_probe --locked
python scripts/test-vps-aws.py --bundle target/vps-aws-bundle `
  --resources target/vps-aws-resources.json --live `
  --rust-probe target/debug/examples/aws_credentials_probe.exe
```

The Rust regression uses synthetic credentials and a loopback SQS fixture. It
checks both profiles, path quoting, actual signed requests, expiry-driven refresh
and failure when renewal fails. The explicit live smoke captures credentials only
in memory: it checks Windows helper/STS identities, real SDK profile loading,
read-only Cognito/SQS access, cross-workload denial and Linux helper issuance inside
non-root/read-only AMD64 DAL images. It never sends/receives production jobs or
changes users. Use `--images <server> <worker>` for the exact approved image IDs.
Run this before and after final VPS installation. The remaining VPS canary is real
API/worker startup with these mounts, egress/TLS, healthy queue polling and graceful
restart, after the separately approved database and provider rollout.

## Dead-letter handling and cost

The source retains its four-day retention and existing encryption/deduplication.
`dal-jobs-dlq.fifo` retains messages for 14 days, uses SQS-managed encryption and
allows redrive only from `dal-jobs.fifo`. Five deliveries allow transient failures
with the worker's 120-second visibility lease (renewed during long jobs). Malformed
JSON is still deleted by existing worker behavior; this setup does not change that.
DLQ movement unblocks the FIFO group: ordering across a failed message and later
replayed messages is not preserved. After fixing the cause, a privileged operator
must review/redrive deliberately; email jobs remain at-least-once and can duplicate
after a crash. Neither runtime role is granted DLQ administration or redrive access.

`dal-jobs-dlq-visible` watches visible messages >= 1, maximum over 60 seconds,
one evaluation, missing data non-breaching. It is console-visible only until an
outbound notification destination is separately approved; an OK/empty alarm is not
proof of notification delivery. No test message is injected into production.

IAM Roles Anywhere/self-managed CA have no service fee. Verified Frankfurt pricing
on 2026-11-02: one standard alarm $0.10/month and FIFO requests $0.50/million before
applicable free tier, taxes and transfer charges. No Private CA, customer KMS key,
SNS topic, new runtime role or persistent AWS access key is required.

AWS references: [certificate credential helper and checksums](https://docs.aws.amazon.com/rolesanywhere/latest/userguide/credential-helper.html),
[PKI trust conditions](https://aws.amazon.com/blogs/security/best-practices-for-integrating-iam-roles-anywhere-with-your-pki/),
[FIFO dead-letter behavior](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-dead-letter-queues.html),
[CloudWatch pricing](https://aws.amazon.com/cloudwatch/pricing/) and [SQS pricing](https://aws.amazon.com/sqs/pricing/).

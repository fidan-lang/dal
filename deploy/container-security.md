# Private production images

The public DAL source does not authorize public prebuilt server/worker images.
Nothing in this procedure publishes images or deploys DAL automatically.

## Reproduce the local acceptance gate

Build both Dockerfile targets from one reviewed commit. Use separate local tags,
not GHCR. The builder and Debian runtime are pinned by multi-platform digest;
`apt-get upgrade` consumes current signed Debian security repositories, so retain
actual image IDs, package versions and scan date rather than claiming byte-for-byte
rebuilds. Do not replace Debian with an image that conceals its package metadata.

Run `node scripts/test-local.mjs --with-cognito`, then (sequentially, since the
Compose fixture ports are shared) run `node scripts/test-images.mjs --with-cli`.
Set `FIDAN_BIN` to the complete published Fidan 1.1.0 installation and set
`DAL_SERVER_IMAGE`/`DAL_WORKER_IMAGE` to the local image tags. The latter gate tests
anonymous concurrent package installs, streamed-download admission/abort/deadline
behavior, read-only/non-root execution, 512 MiB/one-CPU limits, database restore,
real health probes and graceful termination. Worker health expires without
successful SQS and PostgreSQL progress; it is not merely a PID check. A Docker
unhealthy status alone does not restart a container; alert/response policy must be
configured by the operator. It does not prove Mailjet delivery or maintenance-task
health. Allow the existing 180-second worker stop grace in production.

Generate current evidence (all output is local and ignored):

```powershell
./scripts/scan-images.ps1 -ServerImage dal-server:review -WorkerImage dal-worker:review `
  -Syft /path/to/syft -Grype /path/to/grype -Output target/image-security
```

Fetch the official Debian tracker JSON from
<https://security-tracker.debian.org/tracker/data/json> into
`target/debian-tracker.json`, then run:

```sh
python scripts/disposition-images.py --input target/image-security --debian-tracker target/debian-tracker.json
python scripts/test-image-dispositions.py
python scripts/test-private-ghcr.py
```

Retain both SBOM formats, raw findings, scanner DB identity, per-package/CVE
assessment, tracker hash, image identity and runtime-library evidence. The tool
marks only Debian's explicit resolved/fixed-version-zero status as vendor-not-
affected. Upstream binary/CPE matches can also be classified as backport-verified
only after package ownership, `dpkg --verify` integrity and native Debian version
comparison succeed inside the exact network-isolated image. Other findings remain
blocked pending a fix or written risk acceptance. A vendor fix version alone is
not proof the installed package contains it. Review each
affected API against actual `ldd` output and application call paths. DAL's Rust
archive handling does not launch system gzip/perl/tar/mount utilities; this reduces
some paths but does not make all installed OS vulnerabilities harmless. Non-root,
read-only, cap-drop and no-new-privileges mitigate local privilege escalation;
they do not remediate libc or other remotely reachable defects. Keep every raw
finding, including inactive optional Rust lockfile advisories, visible separately.
No source change constitutes residual-risk acceptance.


## Current residual-risk disposition (2026-10-28)

The current local amd64 server and worker builds each have **176 raw scanner
matches: 0 Critical, 61 High, 51 Medium, 12 Low, 52 Negligible**, covering 69 CVE IDs.
All matches remain visible. Grype's two NVD gzip binary matches
(CVE-2026-41991 and CVE-2026-41992) are already backported in Debian
`gzip=1.13-1+deb13u1`: exact-image ownership, executable integrity and native dpkg
version comparisons verify the vendor fix. They are not newly fixed by this PR.
The remaining 67 IDs have no fixed trixie package in the inspected signed package
repositories; raw reports record upstream fix availability and Debian's status,
including cases where upstream fixes exist but stable backports do not. Do not
mix unstable-distribution packages into production to silence a scanner.

| Affected sources | Assessed exposure / required disposition |
| --- | --- |
| glibc | Loaded in both processes. Findings involve strfmon, legacy DNS-printing, tdelete, iconv, wordexp, glob/regex, resolver configuration, loader and platform-specific routines. No network exploit is established here; resolver/loader paths need especially careful acceptance. Preserve DNS/configuration trust, no-new-privileges and restricted environment. **Owner security decision or patched stable package required.** |
| gcc-14 / libstdc++ | Findings concern C++ aligned allocation and priority-queue erase. Captured ldd links libgcc_s, not libstdc++; DAL Rust/AWS-LC code does not call those C++ APIs. Installed-source scanner matches remain recorded; verify again whenever native dependencies change. |
| util-linux, acl, attr, shadow, pam, systemd | Primarily local mount/namespace/permission/authentication tools or daemons. DAL does not invoke those tools or run systemd/PAM authentication. UID 10001, read-only root, no-new-privileges and cap-drop reduce paths; do not equate mitigation with a patch. |
| perl, ncurses, bzip2, gzip, zlib, sqlite3 | DAL uses Rust archive/SQL code and PostgreSQL, does not spawn Perl/infocmp/recovery utilities or link libz/libsqlite. Keep metadata matches and upstream/backport status visible; gzip exceptions are evidence-bound as above. |
| apt, coreutils, dash, diffutils, tar | Runtime API does not invoke package managers, shells or OS archive tools on package data. No hosted extraction path through these utilities was found; keep build-time and local execution exposure separate. |

These are **proposed reachability dispositions, not accepted residual risks**.
There is no blanket waiver for High/Medium findings. Generate the per-ID/package
report above for every release and obtain a recorded owner decision before
production. Container source review/merging is separate from deployment approval.

## Owner-only GHCR checklist (before any publication)

1. Independently inspect **both** `dal-server` and `dal-worker` packages in the
   organization: visibility must explicitly be **Private**. An API 404 proves
   neither existence nor private access. If packages do not exist or cannot be
   inspected, stop; initial private package provisioning needs separate approval.
2. Disable inappropriate repository permission inheritance. Review all explicit
   users, teams and Actions repository access. Only the owner and approved build/
   deployment principals should have access. Public source read access must never
   imply image pull access. Keep automatic public-repository source linkage and
   public build caches out of the image publication path.
3. Configure the `ghcr-release` environment for main only, independent active
   organization-owner review, no self-review and no administrator bypass. Review
   dedicated package-read inspection, minimum publish and read-only deployment-pull
   credentials without copying their values into issues or reports. Record the
   exact package-ID-bound `GHCR_PRIVATE_ACCESS_REVIEW` attestation only after ACL
   inspection; REST visibility alone cannot prove the complete ACL.
4. After a **separate** publication authorization, the manual `ghcr-manual.yaml`
   preflight must pass before login/push. Never publish an image to discover its
   visibility. No PR or normal main push may publish images.
5. After approved publication, verify explicit private visibility again, anonymous
   token + manifest/pull denial and successful authorized read-only pulls. Record
   immutable digests, signatures/provenance and SBOMs. Deploy only `@sha256` refs;
   retain the previous approved digests and protected configuration for rollback.

Production approval additionally requires current CVE disposition, real provider
TLS/IAM canaries (Cognito, PostgreSQL, SQS, R2 and Mailjet), backup/restore and
monitoring on the intended host. Local emulators do not verify those settings.

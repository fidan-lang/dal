<div align="center">

<p align="center">
  <img src="./frontend/static/favicon.svg" width="40%" alt="Dal Banner">
</p>

# Dal

**The official package registry for the [Fidan](https://github.com/fidan-lang/fidan) programming language.**

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE) &nbsp; ![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20macOS-lightgrey.svg)

[Overview](#overview) • [Architecture](#architecture) • [Getting Started](#getting-started) • [API Reference](#api-reference) • [Contributing](#contributing)

</div>

---

## Overview

Dal is the package registry for the Fidan language — it lets developers publish, discover, and install Fidan packages. It follows a sparse-index protocol similar to crates.io and is built with a Rust backend, PostgreSQL, S3-compatible object storage such as Cloudflare R2 (package storage), AWS Cognito (auth), AWS SQS (background jobs), and a SvelteKit frontend.

---

## Architecture

````
┌──────────────────────────────────────────────────┐
│  SvelteKit Frontend  (Cloudflare Pages)           │
└────────────────────┬─────────────────────────────┘
                     │ HTTP
┌────────────────────▼─────────────────────────────┐
│  dal-server  (Axum 0.8, 0.0.0.0:8080)            │
│  Routes: /auth  /packages  /users  /tokens        │
│          /search  /index/{name}  /health          │
└──┬──────────────────┬──────────────┬─────────────┘
   │ sqlx             │ Storage      │ SQS
   ▼                  ▼              ▼
PostgreSQL        dal-packages-  dal-jobs.fifo
                  prod (R2)      ──────────────
                                 dal-worker
                                 (email, etc.)
````

### Crates

| Crate | Purpose |
|---|---|
| ``dal-common`` | Shared error types, pagination, tracing init |
| ``dal-auth`` | Cognito client, JWT validation, API token hashing |
| ``dal-db`` | PostgreSQL queries, migrations, models |
| ``dal-manifest`` | ``dal.toml`` parsing and validation |
| ``dal-storage`` | S3-compatible package archive upload/download |
| ``dal-index`` | Sparse NDJSON index served at ``/index/{name}`` |
| ``dal-server`` | Axum HTTP API server (binary) |
| ``dal-worker`` | SQS background job worker — sends emails (binary) |

---

## Getting Started

### Prerequisites

- Rust (stable, edition 2024)
- Docker (for local Postgres)
- Node.js 24 (CI baseline; compatible Vite 7 requires Node 20.19+ or 22.12+)
- AWS account with Cognito and SQS configured, plus an S3-compatible object store such as Cloudflare R2 (or LocalStack for local dev)

### 1. Clone

```bash
git clone https://github.com/fidan-lang/dal.git
cd dal
```

### 2. Configure environment

```bash
cp .env.example .env
# Fill in AWS credentials, Cognito pool/client IDs, storage bucket, SQS URL
# For Cloudflare R2, use DAL_STORAGE_ACCESS_KEY_ID / DAL_STORAGE_SECRET_ACCESS_KEY
# instead of overriding the global AWS_* credentials used by task-role-backed AWS services.
```

### 3. Start Postgres

```bash
docker compose up postgres -d
```

### 4. Run the server

```bash
cargo run -p dal-server
```

### 5. Run the worker (optional for local dev)

```bash
cargo run -p dal-worker
```

### 6. Run the frontend

```bash
cd frontend
npm ci
npm run dev
```

Set `PUBLIC_API_BASE=http://localhost:8080` explicitly for local frontend development before `npm run dev`. Without it the client defaults to the public API; there is no automatic local proxy. The dev server listens at `http://localhost:5173`.

### 7. Run the test stack

For the full backend integration suite, use the provided test scripts instead of bare
`cargo test` so Postgres, LocalStack, and Cognito-local are started consistently.

```bash
./scripts/test.sh
./scripts/test.sh --with-cognito
```

On Windows:

```powershell
scripts\test.bat
scripts\test.bat --with-cognito
```

### 8. Isolated local release gate

```bash
npm ci --prefix frontend
cd frontend
npx playwright install chromium
cd ..
node scripts/test-local.mjs --with-cognito --e2e
```

This starts a dedicated disposable PostgreSQL/LocalStack project, private Cognito emulator,
API, worker and mail sink using fake credentials and literal IPv4 loopback. It ignores production `.env` files,
requires free local ports, and cleans up only its own processes/containers. The browser
lifecycle uses the sibling `../fidan/target/debug/fidan` (`.exe` on Windows) and local
`../args` candidate; build the matching compiler first. Captured emails contain local
verification/reset tokens; keep `target/audit-logs/` private and ignored. Browser tests use the Playwright-pinned Chromium revision, matching CI; `DAL_BROWSER_CHANNEL=chrome` explicitly selects system Chrome for comparison.

For built local OCI images, run `node scripts/test-images.mjs`. It verifies non-root,
read-only execution, health/readiness and clean shutdown against disposable services.
This is an idle smoke check, not production capacity certification.

### 9. Install Git hooks

To automatically run `cargo fmt --all` before each commit for staged Rust files:

```bash
./scripts/install-git-hooks.sh
```

On Windows:

```powershell
scripts\install-git-hooks.ps1
```

---

## API Reference

API listening uses `DAL_LISTEN_ADDR`. `DAL_BASE_URL` is the frontend origin used for links/CORS; it does not change API route paths.

### Auth

| Method | Path | Description |
|---|---|---|
| ``POST`` | ``/auth/register`` | Register a new account |
| ``POST`` | ``/auth/resend-verification`` | Request a new verification message |
| ``POST`` | ``/auth/login`` | Log in, sets httpOnly cookies |
| ``POST`` | ``/auth/logout`` | Invalidate session |
| ``POST`` | ``/auth/refresh`` | Refresh access token |
| ``GET``  | ``/auth/verify-email`` | Verify email with token from link |
| ``POST`` | ``/auth/forgot-password`` | Request password reset email |
| ``POST`` | ``/auth/reset-password`` | Set new password with reset token |
| ``GET``  | ``/auth/me`` | Get current authenticated user |

### Packages

| Method | Path | Description |
|---|---|---|
| ``GET``  | ``/packages`` | List all packages (paginated) |
| ``GET``  | ``/packages/{name}`` | Get package metadata |
| ``GET``  | ``/packages/{name}/versions`` | List versions |
| ``GET``  | ``/packages/{name}/versions/{version}`` | Get specific version |
| ``GET``  | ``/packages/{name}/versions/{version}/download`` | Download archive |
| ``POST`` | ``/packages/{name}/publish`` | Publish a new version |
| ``PUT``  | ``/packages/{name}/versions/{version}/yank`` | Yank a version |
| ``PUT``  | ``/packages/{name}/versions/{version}/unyank`` | Un-yank a version |

### Owners

| Method | Path | Description |
|---|---|---|
| ``GET``    | ``/packages/{name}/owners`` | List package owners |
| ``POST``   | ``/packages/{name}/owners/invite`` | Invite a collaborator |
| ``DELETE`` | ``/packages/{name}/owners/{username}`` | Remove an owner |
| ``POST``   | ``/packages/{name}/transfer`` | Transfer ownership |

### Invitations and users

| Method | Path | Description |
|---|---|---|
| ``GET`` | ``/owners/invites`` | List pending invitations for the verified session |
| ``POST`` | ``/owners/invites/{id}/accept`` | Accept an authorized pending invitation |
| ``POST`` | ``/owners/invites/{id}/decline`` | Decline an authorized pending invitation |
| ``GET`` | ``/users/{username}`` | Public profile (not private account fields) |
| ``GET`` | ``/users/{username}/packages`` | Public package list |
| ``PATCH`` | ``/users/me/profile`` | Authenticated profile update |

### Tokens

| Method | Path | Description |
|---|---|---|
| ``GET``    | ``/tokens`` | List your API tokens |
| ``POST``   | ``/tokens`` | Create a new API token |
| ``DELETE`` | ``/tokens/{id}`` | Revoke a token |

Token creation/revocation requires a verified interactive session; publishing tokens do not substitute for it.

### Search & Index

| Method | Path | Description |
|---|---|---|
| ``GET`` | ``/search?q=&page=&per_page=`` | Full-text package search |
| ``GET`` | ``/index/{name}`` | Sparse index NDJSON for a package |

### Misc

| Method | Path | Description |
|---|---|---|
| ``GET`` | ``/health`` | Liveness check |
| ``GET`` | ``/readyz`` | Readiness check (includes DB) |
| ``DELETE`` | ``/admin/packages/{name}`` | Administrator-only deletion; not a normal owner operation |

---

## Package contract and relaunch status

The canonical client is `fidan dal`. Package names such as `args` need no `fidan-`
prefix. This local relaunch candidate accepts optional `[package].fidan` SemVer
requirements; omission preserves legacy compatibility. The matching client validates
compiler compatibility and SHA-256 before installation, resolves intersecting graphs
with bounded backtracking, and records checksums in schema-1 `dal.lock` files.
New resolution excludes yanked versions; an existing exact checksum lock may reinstall
its yanked version. Immutable archives remain downloadable. A checksum authenticates
bytes relative to the trusted HTTPS registry/index, not a compromised registry.

Archives are limited to 50 MiB compressed, 256 MiB expanded, 100:1 expansion and 10,000
entries. Only regular files/directories with portable relative paths are accepted.
Publishing holds a package-scoped database lock and uses per-attempt object keys;
unknown commit outcomes retain the object for reconciliation. API uploads are limited
to two concurrent archive operations per process; lower `DAL_MAX_UPLOAD_BYTES` if needed.

No production relaunch or public `args` package is claimed by this branch.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and pull request guidelines.

---

## License

DAL is licensed under the standard [Apache License 2.0](LICENSE), with attribution
in [NOTICE](NOTICE). Commercial use, modification, redistribution, and hosting
are permitted, including independent and competing commercial registries.

The official public DAL registry is intended to be free to use for package
installation, publishing, dependency resolution, caching, and legitimate CI/CD.
Package authors retain their rights; each package's own license governs reuse
and redistribution.

See [TRADEMARKS.md](TRADEMARKS.md) for separate branding guidance and
[REGISTRY_TERMS.md](REGISTRY_TERMS.md) for the official hosted service's draft
acceptable-use terms, which require legal review before publication/enforcement.

Copyright © 2026 Kaan Gönüldinc (AppSolves)

## Release and image policy

Official prebuilt `ghcr.io/fidan-lang/dal-server` and `dal-worker` images must remain
**private**, with no anonymous pulls or inherited public-repository access. The
Apache-2.0 source remains available for independent builds and hosting.
See [private image release requirements](CONTRIBUTING.md#private-image-release-requirements).

Production DAL must be approved and operational after Fidan 1.1.0 stable and its
protocol-8 LLVM helper are released, and **before** the first args 0.1.0 publication.
No production launch or package/image publication follows automatically from CI.

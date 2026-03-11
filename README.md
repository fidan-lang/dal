<div align="center">

# Dal

**The official package registry for the [Fidan](https://github.com/fidan-lang/fidan) programming language.**

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE) &nbsp; ![Build](https://img.shields.io/badge/build-passing-brightgreen.svg) &nbsp; ![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows%20%7C%20macOS-lightgrey.svg)

[Overview](#overview) • [Architecture](#architecture) • [Getting Started](#getting-started) • [API Reference](#api-reference) • [Contributing](#contributing)

</div>

---

## Overview

Dal is the package registry for the Fidan language — it lets developers publish, discover, and install Fidan packages. It follows a sparse-index protocol similar to crates.io and is built with a Rust backend, PostgreSQL, AWS S3 (package storage), AWS Cognito (auth), AWS SQS (background jobs), and a SvelteKit frontend.

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
   │ sqlx             │ S3           │ SQS
   ▼                  ▼              ▼
PostgreSQL        dal-packages-  dal-jobs.fifo
                  prod (S3)      ──────────────
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
| ``dal-storage`` | S3 package archive upload/download |
| ``dal-index`` | Sparse NDJSON index served at ``/index/{name}`` |
| ``dal-server`` | Axum HTTP API server (binary) |
| ``dal-worker`` | SQS background job worker — sends emails (binary) |

---

## Getting Started

### Prerequisites

- Rust (stable, edition 2024)
- Docker (for local Postgres)
- Node.js 20+ (for frontend)
- AWS account with Cognito, S3, SQS configured (or LocalStack for local dev)

### 1. Clone

```bash
git clone https://github.com/fidan-lang/dal.git
cd dal
```

### 2. Configure environment

```bash
cp .env.example .env
# Fill in AWS credentials, Cognito pool/client IDs, S3 bucket, SQS URL
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
npm install
npm run dev
```

The frontend dev server starts at ``http://localhost:5173`` and proxies API calls to ``http://localhost:8080``.

---

## API Reference

All endpoints are under the base URL configured in ``.env`` (``DAL_BASE_URL``).

### Auth

| Method | Path | Description |
|---|---|---|
| ``POST`` | ``/auth/register`` | Register a new account |
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

### Tokens

| Method | Path | Description |
|---|---|---|
| ``GET``    | ``/tokens`` | List your API tokens |
| ``POST``   | ``/tokens`` | Create a new API token |
| ``DELETE`` | ``/tokens/{id}`` | Revoke a token |

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

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and pull request guidelines.

---

## License

Licensed under the [Apache License 2.0](LICENSE).

Copyright © 2026 Kaan Gönüldinc (AppSolves)

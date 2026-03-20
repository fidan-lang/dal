# Contributing to Dal

Thank you for your interest in contributing to **Dal** — the package registry for the Fidan programming language. Contributions help improve the registry server, worker, frontend, and ecosystem. We welcome bug fixes, performance improvements, documentation updates, and new features.

Please read this document before submitting a contribution.

---

## Development Setup

### 1. Clone the repository

```bash
git clone https://github.com/fidan-lang/dal.git
cd dal
```

### 2. Start local dependencies

```bash
docker compose up postgres -d
```

### 3. Build the workspace

```bash
cargo build
```

### 4. Run the server

```bash
cargo run -p dal-server
```

### 5. Run the frontend

```bash
cd frontend
npm install
npm run dev
```

Before submitting a pull request, make sure the project builds successfully and all relevant tests pass.

---

## Project Structure

The repository is organized as a Cargo workspace with a SvelteKit frontend.

```text
crates/
    dal-common/     # shared error types, pagination, tracing
    dal-auth/       # Cognito JWT validation, API token hashing
    dal-db/         # PostgreSQL queries and migrations
    dal-manifest/   # dal.toml parsing and validation
    dal-storage/    # S3-compatible package archive storage
    dal-index/      # sparse package index (NDJSON)
    dal-server/     # Axum HTTP API server
    dal-worker/     # SQS background job worker
frontend/           # SvelteKit UI (Cloudflare Pages)
migrations/         # sqlx SQL migration files
```

---

## Contribution Guidelines

Please follow these guidelines when contributing:

- Keep pull requests **focused and minimal**
- Include **tests whenever possible**
- Write **clear and descriptive commit messages**
- Follow the existing code style and architecture
- Avoid unrelated refactoring in the same pull request
- Keep changes easy to review

Large architectural changes, language design changes, or major runtime/compiler changes should be discussed in an **issue before implementation**.

If you are unsure whether something fits the project direction, open an issue first before investing large amounts of time.

---

## Branching Rules

Do **not commit directly to `main`**.

All contributions must be made from a separate branch.

Example workflows:

```bash
git checkout -b feature/my-improvement
```

```bash
git checkout -b fix/parser-bug
```

Use descriptive branch names that reflect the purpose of the change.

Recommended prefixes:

- `feature/`
- `fix/`
- `docs/`
- `perf/`
- `refactor/`

---

## Code Style

Fidan follows standard Rust conventions.

Before submitting a pull request, run:

```bash
cargo fmt
cargo clippy
```

Code should compile cleanly and avoid unnecessary warnings whenever possible.

Please try to match the style and structure already used in the surrounding code. Consistency is more important than personal style preferences.

---

## Pull Request Process

1. Fork the repository
2. Create a new branch for your change
3. Implement your changes
4. Run formatting and tests
5. Open a pull request

Pull requests should include:

- A clear description of the change
- Motivation for the change
- Any relevant issue references
- Tests if applicable
- Notes about limitations or unfinished parts if relevant

All pull requests must pass CI before they can be merged.

Pull requests that mix multiple unrelated changes may be asked to be split into smaller PRs.

---

## Commit Messages

Please write commit messages that clearly explain the purpose of the change.

Good examples:

- `parser: fix precedence handling for null-coalescing operator`
- `runtime: reduce allocation overhead in bytecode VM`
- `docs: add syntax examples for extension actions`

Avoid vague commit messages like:

- `fix stuff`
- `update`
- `changes`

---

## Tests

If your contribution changes behavior, please add or update tests whenever practical.

Relevant test categories may include:

- lexer tests
- parser tests
- semantic analysis tests
- runtime/interpreter tests
- language server/editor tests

Bug fixes should ideally include a regression test so the issue does not return later.

---

## Documentation

If you introduce new syntax, change behavior, or modify developer workflows, please update the relevant documentation as part of the same pull request when possible.

This may include:

- `README`
- language documentation
- architecture notes
- editor/tooling documentation

---

## Contributor License Agreement (CLA)

Before a pull request can be merged, contributors must sign the **Fidan Contributor License Agreement (CLA)**.

The CLA ensures that contributions can legally be included in the Fidan project and distributed under the project’s license.

Details will be provided during the pull request process.

---

## Reporting Bugs

If you encounter a bug, please open a GitHub issue and include:

- Fidan version
- Operating system
- Minimal reproduction code
- Expected behavior
- Actual behavior
- Any relevant logs or diagnostics

Clear reproduction steps make issues much easier to investigate.

---

## Feature Requests

Feature proposals should include:

- Motivation for the feature
- A short design overview
- Potential impact on the language or tooling
- Examples of intended usage
- Possible tradeoffs if relevant

Major language features should always be discussed before implementation.

---

## Security

If you discover a security issue, please avoid posting exploit details publicly before the issue can be assessed.

If a dedicated security policy exists later, follow that process. Until then, report security-sensitive issues responsibly.

See [SECURITY.md](SECURITY.md) for more details.

---

## Code of Conduct

Please be respectful, constructive, and professional when interacting with other contributors.

Fidan aims to maintain a welcoming and high-quality development environment.

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for more details.

---

## Final Note

By contributing to Fidan, you help improve the language and its ecosystem for everyone. Thank you for your contribution.

---
name: Bug report
about: Something in Dal is behaving incorrectly
title: "[bug] "
labels: bug
assignees: AppSolves
---

## Describe the bug

<!-- A clear and concise description of what is wrong. -->

## Affected area

<!-- Delete anything that does not apply. -->

- Registry API
- Package publishing
- Package downloads
- Sparse index
- Authentication
- Worker / background jobs
- Frontend
- Database / migrations
- Storage
- Documentation

## Reproduction steps

<!-- Provide the smallest set of steps needed to reproduce the problem. -->

```bash
# commands, curl requests, or local setup steps
```

```toml
# dal.toml, package metadata, or config involved, if relevant
```

## Expected behaviour

<!-- What you expected to happen. -->

## Actual behaviour

<!-- What actually happened. Paste API responses, browser errors, panic traces, or wrong output. -->

## Environment

- **Dal version / commit**:
- **Fidan version** (`fidan --version`, if relevant):
- **OS / architecture**:
- **Rust version** (`rustc -V`, if running locally):
- **Node.js version** (`node --version`, if frontend-related):
- **Backend services** (Postgres / LocalStack / Cognito-local / R2 / SQS):

## Logs

<!-- Paste relevant server, worker, browser console, or test output. Remove secrets and tokens. -->

```text

```

## Additional context

<!-- Anything else that might help: screenshots, related packages, request IDs, links, or recent changes. -->

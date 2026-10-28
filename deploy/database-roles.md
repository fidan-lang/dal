# Database roles and row security

This is the activation procedure for migration `0012_runtime_row_security.sql`.
It has not been applied to Neon. A live database with migrations 0001–0009 still
needs 0010, 0011 and 0012 under separate operator approval.

## Runtime contract

- `dal_api` and `dal_worker` are separate LOGIN, NOINHERIT, NOSUPERUSER,
  NOBYPASSRLS, NOCREATEDB, NOCREATEROLE, NOREPLICATION roles with no memberships.
  They own no tables, schemas or functions and cannot create objects in the
  protected schemas. Passwords are never shipped in the migration.
- The controlled migration owner remains the owner, including the BYPASSRLS
  required by narrowly defined service functions. Neither runtime inherits it or
  `neon_superuser`. Normal API/worker startup rejects an owner, a role membership,
  elevated flags, database CREATE/TEMP privileges or an incomplete RLS schema. Startup does **not** run migrations.
- Verified JWT/API-token authentication binds actor, credential class and actual
  API-token ID with `set_config(..., true)` on the **same SQLx transaction** as the
  operation. Commit/rollback/drop resets the context before pool reuse. Request
  bodies, proxy headers and arbitrary user IDs never establish HTTP identity.
- Custom PostgreSQL settings are mutable by SQL. This RLS layer catches missing
  predicates and cross-user query mistakes; it is **not** a sandbox for arbitrary
  SQL execution or a compromised API process. Rust JWT/MFA validation, scopes,
  ownership, last-owner checks and transaction/locking rules remain authoritative.
  Credential administration and MFA records require session context; an API token
  cannot become an administrator just because its owner is one.

| Tables | Runtime access |
| --- | --- |
| users | API reads its actor (or verified session admin), updates only granted profile/revocation columns (API profile writes additionally require user:write). No direct runtime insert, privilege/email-verification edits or API delete. Worker reads/deletes only unverified accounts older than one day, with the existing stricter cleanup cutoff rechecked under a row lock. |
| api_tokens | Only the actor's verified session can list/create/delete. No direct updates. Authentication uses an exact token-hash capability and bounded touch operation. |
| verification_tokens | No direct runtime reads/writes. Issue/consume functions preserve expiry, single-use consumption and transactional email verification. |
| owner_invites | Invitee sessions and authorized package owners have scoped visibility. Owners issue/change invitations; invitees cannot rewrite role, target, inviter, hash or expiry. Acceptance rechecks inviter authority under the existing package lock. |
| user_mfa, mfa_sessions, mfa_id_tokens | Only the owning verified session has row access. Pre-authentication checks return a boolean, never all proof records. Existing revocation and MFA binding locks remain. |
| packages, package_versions, package_owners | Public reads remain intentional. Authenticated mutations require ownership/membership and relevant credential scopes. First-owner bootstrap is restricted to the creator's new transaction. Archive identities cannot be updated by the runtime. |
| download_logs | No direct runtime reads; only narrow accounting and aggregate-chart functions. Public download accounting checks that version and package match. |
| audit_log | Append-only for the authenticated actor; verified session admins can read. Runtime cannot update/delete events. |

All twelve tables enable and force RLS. Sensitive tables have no universal
`USING (true)` policy. The only universal row reads are intentionally public
package/version/ownership metadata. Public user profiles are a projection excluding
email, Cognito subject, admin flags and revocation state. Fixed security-definer
functions have a pinned `pg_catalog, pg_temp` search path, schema-qualified tables,
no dynamic user SQL, revoked PUBLIC execution and explicit service-role grants.
The API deliberately has narrowly parameterized pre-authentication account lookup,
registration and recovery capabilities; do not describe those as user-scoped CRUD.

## Disposable development setup

These credentials are **local fixtures only**, never production credentials:

```sh
docker compose up -d postgres localstack
DAL_MIGRATION_DATABASE_URL=postgres://dal:password@localhost:5432/dal   cargo run --locked -p dal-server -- --migrate
docker compose exec postgres psql -U dal -d dal -v ON_ERROR_STOP=1   -c "ALTER ROLE dal_api PASSWORD 'dev-api'; ALTER ROLE dal_worker PASSWORD 'dev-worker';"
```

Then the development Compose API/worker use their distinct local role URLs.
For native development use `DATABASE_URL=postgres://dal_api:dev-api@localhost:5432/dal`
for the API and `postgres://dal_worker:dev-worker@localhost:5432/dal` for the worker.
The isolated `scripts/test-local.mjs --with-cognito --e2e` provisions its **own**
loopback-only fixture roles/passwords; no real configuration is used. Fixture setup
and fault injection use an explicit owner pool; actual routers run as `dal_api`,
and the actual browser-test worker runs as `dal_worker`.

## Production activation: separately approved, not executed here

1. Review the migration, role/function ownership, table policies and exact image
   digests independently. Take a protected database backup and rehearse restoration
   on an isolated approved database. Provision the same role names before restore;
   database dumps do not contain cluster roles/passwords. Preserve ACLs and policy
   grants (`pg_restore --no-owner`, **not** `--no-acl`), with a controlled restore
   owner that can own the security-definer functions. Re-provision credentials
   independently. Database-level ACLs are not included in ordinary schema/data
   dumps: explicitly revoke PUBLIC CREATE/TEMP on the isolated restore database
   and grant CONNECT to the runtime roles before testing it. Read back roles,
   grants, policies and authenticated behavior.
2. Inspect the selected Neon branch/database and migration journal. Confirm the
   current controlled owner has the CREATEROLE authority needed for first-time
   provisioning, table ownership and BYPASSRLS. If provider permissions prevent
   role creation, obtain administrator provisioning of the two exact restricted
   roles; do not give those privileges to the runtime. The migration refuses unsafe
   pre-existing roles rather than silently changing their privilege configuration.
3. Stop API/worker writes under an approved maintenance window. Provide the owner
   connection only to the one-off command, via protected environment injection:

   ```sh
   cargo run --locked -p dal-server -- --migrate
   ```

   `DAL_MIGRATION_DATABASE_URL` is mandatory for this mode; it never falls back to
   `DATABASE_URL`. For a reviewed server image the equivalent command is
   `/usr/local/bin/dal-server --migrate`. Do not place the owner credential in either
   long-running container's environment. Do not log or paste passwords/URLs.
4. Assign distinct runtime secrets through the approved secret mechanism (interactive
   `psql` `\password dal_api` / `\password dal_worker` is an option without passwords
   in command history). Require the approved verified TLS configuration; test the
   actual Neon pooler/direct endpoint and its transaction-local settings. No such
   provider setting is inferred from local PostgreSQL tests.
5. Supply `DAL_API_ENV_FILE` and `DAL_WORKER_ENV_FILE` to
   `deploy/compose.production.yaml`, each with its own `DATABASE_URL` and only that
   process's provider credentials. The old shared `DAL_ENV_FILE` is no longer used.
   Keep owner credentials off the host runtime path. Verify flags/memberships:

   ```sql
   SELECT rolname,rolsuper,rolbypassrls,rolinherit,rolcreatedb,rolcreaterole,rolreplication
     FROM pg_roles WHERE rolname IN ('dal_api','dal_worker');
   SELECT r.rolname,p.rolname AS member_of FROM pg_auth_members m
     JOIN pg_roles r ON r.oid=m.member JOIN pg_roles p ON p.oid=m.roleid
     WHERE r.rolname IN ('dal_api','dal_worker'); -- must be empty
   SELECT c.relname,c.relrowsecurity,c.relforcerowsecurity,pg_get_userbyid(c.relowner)
     FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
     WHERE n.nspname='public' AND c.relkind='r';
   SELECT * FROM pg_policies WHERE schemaname='public';
   ```

6. Run approved canaries for anonymous installs, two independent accounts, scoped
   publishing/invitations, password recovery, MFA/login/refresh/revocation, worker
   jobs and cleanup. Verify health, 429 behavior, logs, monitoring and restore.
   Never turn off RLS or reconnect the service as owner to make a failed canary pass.
7. On failure, keep services stopped and fix forward, or restore the approved full
   backup/image/configuration together under a separate rollback approval. Do not
   drop MFA guards, remove policies, or run old owner-dependent binaries against
   the new schema as an improvised rollback.

Source merge is not activation approval. Live Neon TLS/IAM, owner privileges,
credential provisioning, pooler behavior and the production rollout remain
**NOT VERIFIED / PENDING OPERATOR APPROVAL**.

# Optional Cognito authenticator MFA

## Production configuration

The DAL production Cognito user pool uses `MfaConfiguration=OPTIONAL` with
software-token MFA enabled. The DAL API has `DAL_TOTP_ENABLED=true`. Accounts
without TOTP can still use password login; enrolled accounts must complete a
Cognito authenticator challenge. Public package downloads remain anonymous.

In deployment, `DAL_TOTP_ENABLED` is injected into the API container with the
production Compose configuration. Restarting an existing container without
re-creation does not update environment variables. Verify the effective value
in the recreated container and confirm `/readyz` reports `status: ready`.

To inspect Cognito configuration independently:

```sh
aws cognito-idp get-user-pool-mfa-config --user-pool-id "$POOL_ID"
```

The selected pool should report `OPTIONAL` and enabled software-token MFA.
Do not change the pool to `ON` or `OFF` for normal operation.

## Authentication and enrollment

- Password login returns `{"mfa_required":true,"expires_in":180}` for enrolled
  users and creates an HttpOnly, SameSite=Strict, three-minute pending challenge
  cookie. The Cognito challenge session is never exposed to the browser.
- `POST /auth/mfa/challenge` validates the six-digit authenticator code with
  Cognito and creates DAL sessions only after completed identity and revocation
  checks. Unenrolled users continue through the regular password flow.
- `/settings/security` supports enrollment, replacement, disabling, and recovery
  code management. MFA changes require a verified interactive session and fresh
  password authentication; replacing or disabling an existing authenticator also
  requires its current code. API tokens cannot administer MFA.
- `POST /auth/mfa/setup` begins enrollment. `POST /auth/mfa/verify` confirms the
  new TOTP factor, stores ten hashed recovery-code grants, and returns their
  plaintext only once for display and download. Existing sessions are revoked
  immediately; users confirm that they saved the codes before navigating to
  sign-in. No authentication is restored by merely viewing the codes.
- Completed enrollment sets DAL's durable downgrade guard and revokes existing
  sessions through its database revocation epoch and Cognito global sign-out.
  Existing MFA grants and replay attempts cannot bypass the guard.
- Authentication and setup responses use `Cache-Control: no-store`. Secrets,
  passwords, recovery codes, and Cognito sessions must not be logged.

MFA challenges are bounded, single-use, and held in process-local memory with a
three-minute lifetime. The production topology uses one API instance; adding
instances requires a shared atomic challenge store or safe authenticated
session affinity. A lost pending challenge requires restarting sign-in.

## Recovery codes

DAL generates **ten random 256-bit, single-use recovery codes** at successful
TOTP enrollment. The codes are shown only in the successful response; only
SHA-256 hashes are stored in `mfa_recovery_codes` under forced RLS. Users should
save the codes offline before leaving the confirmation screen.

An enrolled, signed-in user may rotate codes through `/settings/security` using
both a fresh password and current TOTP code. Rotating codes invalidates all
previously generated recovery codes.

When an authenticator is lost, the user first passes the Cognito password step.
DAL then permits a recovery code to be submitted using the short-lived pending
MFA cookie. A valid recovery code is consumed atomically and does **not** sign
in the user or create a DAL session. Instead, the server requests the Cognito
software-token reset, completes the protected database recovery grant, revokes
existing sessions and API tokens, and requires the user to sign in again and
set up a new authenticator.

This uses migration `0013_mfa_recovery_codes.sql`. The API role has
`cognito-idp:AdminDeleteSoftwareToken` restricted to the DAL user-pool ARN; the
worker has no such permission. Provider errors or uncertain partial outcomes
fail closed. Do not clear the persistent MFA guard to resolve failed recovery.

Password reset or access to a verified email address alone cannot remove MFA.
For exceptional operator-assisted recovery, verify identity independently,
record the incident without secrets, and follow controlled reset and revocation
procedures. Never request a user's password or authenticator secret.

## Operational checks

- Confirm password-only login for a non-enrolled account and challenged login
  for an enrolled account.
- Verify the first-time codes appear after setup, can be saved, and are not
  retrievable again from the server.
- Verify code rotation requires fresh password and TOTP and invalidates old codes.
- Verify a recovery code cannot be reused, does not issue a session, and revokes
  previous sessions and API tokens after factor reset.
- Verify the new authenticator setup and subsequent TOTP-protected login.
- Review authentication logs without exposing passwords, OTPs, or recovery codes.

AWS references: [TOTP behavior](https://docs.aws.amazon.com/cognito/latest/developerguide/user-pool-settings-mfa-totp.html),
[pool configuration](https://docs.aws.amazon.com/cognito-user-identity-pools/latest/APIReference/API_SetUserPoolMfaConfig.html),
[software-token reset](https://docs.aws.amazon.com/cognito-user-identity-pools/latest/APIReference/API_AdminDeleteSoftwareToken.html).

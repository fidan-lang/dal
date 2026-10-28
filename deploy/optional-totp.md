# Optional Cognito authenticator MFA

This is a source rollout procedure, not evidence of a live Cognito change. The
existing pool is expected to remain `MfaConfiguration=OFF` until operator approval.

## Contract

- Password login is unchanged for accounts without MFA. `POST /auth/login` returns
  `{"mfa_required":true,"expires_in":180}` for a software-token challenge. Only an
  HttpOnly, SameSite=Strict, three-minute opaque pending cookie is set. Cognito's
  challenge session is never returned to the browser. `POST /auth/mfa/challenge`
  accepts `{"code":"123456"}` and issues session cookies only after full Cognito
  authentication and DAL identity/revocation checks succeed.
- `/settings/security` shows status, locally generated QR/manual setup, verification,
  replacement and disable. Setup/disable require a verified interactive session,
  freshly checked password and the current code when MFA is enabled. API tokens
  cannot administer MFA. `/auth/mfa/setup`, `/verify`, `/disable` are JSON POSTs;
  `/auth/mfa/status` is a session-only GET. All auth responses are `no-store`.
- Cognito creates, stores and verifies TOTP secrets. DAL retains a persistent
  boolean downgrade guard, hashes binding completed MFA ID/refresh tokens, and
  bounded transient login/setup state (1,024 entries,
  three-minute lifetime, five attempts). Handles are random 256-bit values stored
  only by hash. A handle is consumed before provider I/O and cannot be replayed
  after completion. Passwords, codes, secrets and provider sessions are not logged.
- Successful changes advance DAL's revocation epoch and request Cognito global
  sign-out; existing browser sessions and refreshes are rejected. Existing scoped
  API tokens keep their separate authorization contract. Use a fresh code to sign
  in again. Email verification, password reset and account deletion remain intact;
  resetting a password does not disable MFA. Direct JWTs and refresh tokens without
  a completed DAL MFA grant cannot bypass the durable guard. Revocation removes
  grants atomically.
- Pending state is process-local: restarting loses pending work safely. The
  supported initial topology is one API process. Do not horizontally scale this
  stateful challenge flow without a shared, atomic, expiring challenge store or
  authenticated affinity; failure must restart authentication, never bypass MFA.

## Approved operator rollout (not executed by source tests)

1. Back up the database and record the exact image digests and current Cognito
   pool/client configuration. Review IAM and public-origin controls. Apply migration
   `0011_optional_mfa.sql` using the established migration process. Deploy reviewed
   server/frontend together with `DAL_TOTP_ENABLED=false` first. Confirm existing
   login, refresh, email verification/reset, scoped API tokens and downloads.
2. Read the selected pool with `aws cognito-idp get-user-pool-mfa-config
   --user-pool-id "$POOL_ID"` and the exact app client with
   `describe-user-pool-client`. Do not infer their live settings from this file.
   Preserve the existing client and all unrelated settings. It must support
   USER_PASSWORD_AUTH/refresh and the user access-token scope
   `aws.cognito.signin.user.admin`. Do not enable SMS/email MFA, remembered-device
   MFA suppression or a second fallback factor during this rollout.
3. After explicit provider approval, apply only:

   ```sh
   aws cognito-idp set-user-pool-mfa-config \
     --user-pool-id "$POOL_ID" \
     --mfa-configuration OPTIONAL \
     --software-token-mfa-configuration Enabled=true
   ```

   Review and preserve any existing SMS/email configuration before issuing this
   request. Read back the result: OPTIONAL, software token enabled. Never use ON.
   Keep the client's authentication session validity at three minutes. The server
   uses `AdminGetUser` (already used for registration), and user-authorized
   AssociateSoftwareToken, VerifySoftwareToken, SetUserMFAPreference,
   RespondToAuthChallenge. Those user-authorized operations use Cognito tokens,
   not additional administrator IAM grants. Do not grant reset/recovery admin
   permissions to the server.
4. Set `DAL_TOTP_ENABLED=true` on the reviewed deployment, not in production here.
   Test a disposable approved canary user: password-only before enrollment, invalid
   setup code, verified enrollment, missing/incorrect login code, replay, replacement,
   disable and session revocation. Check a separate non-enrolled user still logs in
   with only a password. Check UI origin/CORS, Secure cookies, no-store and logs.
5. Stop new enrollment with `DAL_TOTP_ENABLED=false` if needed. Existing MFA login
   and authenticated disable must stay supported. Do NOT set the pool OFF or roll
   back to a server that cannot process MFA once any user enrolls. Keep the database
   downgrade guard. Enrollment commits its downgrade guard before provider verification. Definite
   invalid-code responses restore previously disabled enrollment; ambiguous provider
   failures retain the guard and can require operator reconciliation. Provider/DB
   partial failures fail closed; reconcile account
   preference and the durable guard under an approved incident procedure.

## Lost authenticator recovery

Prefer restoring the authenticator from the user's offline backup of the manual
setup secret. Never transmit that backup to support. Password-reset or mailbox
access alone is not MFA recovery. No public endpoint or normal API token can
remove MFA without the current factor.

If no backup exists, use a separately authorized operator-assisted process:
verify identity against independent, previously established organizational or
in-person evidence (not newly supplied documents/contacts or an email reply).
Require an independent operator's approval, record a non-secret incident reference,
notify existing verified contacts, and reject the recovery if independent proof is
unavailable. This can mean the account remains inaccessible; package ownership is
not transferred by a recovery request. Never ask for a password or TOTP secret.

Only after that approval may a restricted operator remove the Cognito software
factor using `AdminDeleteSoftwareToken`, revoke all Cognito sessions, and advance
DAL's `users.session_invalid_before`. Reconcile `user_mfa.required=false` for the
verified account in a reviewed transaction, revoke its API tokens as an incident
response, and require a new password and new enrollment before resuming privileged
work. The runtime principal must NOT have AdminDeleteSoftwareToken permission.
Do not provide an email-only or silent support override. These live recovery steps
require separate authorization and must be rehearsed on a disposable pool first.

AWS references: [TOTP behavior](https://docs.aws.amazon.com/cognito/latest/developerguide/user-pool-settings-mfa-totp.html),
[pool configuration](https://docs.aws.amazon.com/cognito-user-identity-pools/latest/APIReference/API_SetUserPoolMfaConfig.html),
[operator reset](https://docs.aws.amazon.com/cognito-user-identity-pools/latest/APIReference/API_AdminDeleteSoftwareToken.html).

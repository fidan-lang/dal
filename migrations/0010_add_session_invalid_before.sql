-- JWTs are verified locally, so provider sign-out alone cannot revoke an
-- already-issued bearer token at the Dal API. This monotonically advances the
-- per-user cutoff checked against the token's issued-at timestamp.
ALTER TABLE users
    ADD COLUMN session_invalid_before TIMESTAMPTZ;

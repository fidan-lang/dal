-- Recovery codes are independent 256-bit capabilities; Cognito still owns TOTP.
-- This migration must be applied by the owner role, never by dal_api/worker.
CREATE TABLE public.mfa_recovery_codes (
    user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    code_hash text NOT NULL CHECK (length(code_hash)=64),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id,code_hash)
);
CREATE TABLE public.mfa_recovery_grants (
    user_id uuid PRIMARY KEY REFERENCES public.users(id) ON DELETE CASCADE,
    ticket_hash text NOT NULL CHECK (length(ticket_hash)=64),
    expires_at timestamptz NOT NULL
);
ALTER TABLE public.mfa_recovery_codes ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.mfa_recovery_codes FORCE ROW LEVEL SECURITY;
ALTER TABLE public.mfa_recovery_grants ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.mfa_recovery_grants FORCE ROW LEVEL SECURITY;
REVOKE ALL ON public.mfa_recovery_codes, public.mfa_recovery_grants FROM PUBLIC,dal_api,dal_worker;

-- Generation requires a verified interactive session. Rust separately requires
-- fresh password+TOTP authentication; never accept an API token for generation.
CREATE FUNCTION dal_security.replace_recovery_codes(uid uuid, hashes text[]) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE h text;
BEGIN
  IF dal_security.actor() IS DISTINCT FROM uid OR NOT dal_security.session()
     OR NOT dal_security.allowed() THEN
    RAISE EXCEPTION 'interactive session required';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM public.user_mfa WHERE user_id=uid AND required) THEN
    RAISE EXCEPTION 'MFA enrollment required';
  END IF;
  IF array_length(hashes,1)<>10 OR
     (SELECT count(DISTINCT v) FROM unnest(hashes) AS codes(v) WHERE v ~ '^[0-9a-f]{64}$')<>10 THEN
    RAISE EXCEPTION 'ten unique valid recovery hashes required';
  END IF;
  DELETE FROM public.mfa_recovery_codes WHERE user_id=uid;
  FOREACH h IN ARRAY hashes LOOP
    INSERT INTO public.mfa_recovery_codes(user_id,code_hash) VALUES(uid,h);
  END LOOP;
  DELETE FROM public.mfa_recovery_grants WHERE user_id=uid;
END $$;

CREATE FUNCTION dal_security.recovery_code_count(uid uuid) RETURNS bigint
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
  IF dal_security.actor() IS DISTINCT FROM uid OR NOT dal_security.session()
     OR NOT dal_security.allowed() THEN
    RAISE EXCEPTION 'interactive session required';
  END IF;
  RETURN (SELECT count(*) FROM public.mfa_recovery_codes WHERE user_id=uid);
END $$;

-- The caller has already verified the user's password with Cognito and
-- obtained an opaque 3-minute pending MFA cookie. A correct recovery hash can
-- only create one short-lived authorization for resetting that user's factor.
-- Serialize attempts with the user row lock across all API instances.
CREATE FUNCTION dal_security.consume_recovery_code(uid uuid, supplied_hash text, ticket text)
RETURNS boolean LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE consumed text;
BEGIN
  IF supplied_hash !~ '^[0-9a-f]{64}$' OR ticket !~ '^[0-9a-f]{64}$' THEN RETURN false; END IF;
  PERFORM 1 FROM public.users WHERE id=uid AND email_verified FOR UPDATE;
  IF NOT FOUND THEN RETURN false; END IF;
  IF NOT EXISTS (SELECT 1 FROM public.user_mfa WHERE user_id=uid AND required) THEN RETURN false; END IF;
  DELETE FROM public.mfa_recovery_grants WHERE user_id=uid AND expires_at<=now();
  IF EXISTS (SELECT 1 FROM public.mfa_recovery_grants WHERE user_id=uid) THEN RETURN false; END IF;
  DELETE FROM public.mfa_recovery_codes WHERE user_id=uid AND code_hash=supplied_hash
    RETURNING code_hash INTO consumed;
  IF consumed IS NULL THEN RETURN false; END IF;
  INSERT INTO public.mfa_recovery_grants(user_id,ticket_hash,expires_at)
    VALUES(uid,ticket,now()+interval '2 minutes');
  RETURN true;
END $$;

-- Only a matching, unexpired, one-use recovery grant can finish reset. Rust
-- calls this function only AFTER Cognito confirms software-token removal.
CREATE FUNCTION dal_security.finish_recovery(uid uuid, ticket text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE found_ticket text;
BEGIN
  PERFORM 1 FROM public.users WHERE id=uid FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'unknown recovery identity'; END IF;
  DELETE FROM public.mfa_recovery_grants WHERE user_id=uid AND ticket_hash=ticket
    AND expires_at>now() RETURNING ticket_hash INTO found_ticket;
  IF found_ticket IS NULL THEN RAISE EXCEPTION 'recovery authorization invalid'; END IF;
  UPDATE public.user_mfa SET required=false WHERE user_id=uid;
  UPDATE public.users SET session_invalid_before=now(),updated_at=now() WHERE id=uid;
  DELETE FROM public.mfa_sessions WHERE user_id=uid;
  DELETE FROM public.mfa_recovery_codes WHERE user_id=uid;
  DELETE FROM public.api_tokens WHERE user_id=uid;
END $$;

CREATE FUNCTION dal_security.clear_recovery_codes(uid uuid) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
  IF dal_security.actor() IS DISTINCT FROM uid OR NOT dal_security.session()
     OR NOT dal_security.allowed() THEN RAISE EXCEPTION 'interactive session required'; END IF;
  DELETE FROM public.mfa_recovery_codes WHERE user_id=uid;
  DELETE FROM public.mfa_recovery_grants WHERE user_id=uid;
END $$;

REVOKE ALL ON FUNCTION dal_security.replace_recovery_codes(uuid,text[]) FROM PUBLIC;
REVOKE ALL ON FUNCTION dal_security.recovery_code_count(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION dal_security.consume_recovery_code(uuid,text,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION dal_security.finish_recovery(uuid,text) FROM PUBLIC;
REVOKE ALL ON FUNCTION dal_security.clear_recovery_codes(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION dal_security.replace_recovery_codes(uuid,text[]),
  dal_security.recovery_code_count(uuid),
  dal_security.consume_recovery_code(uuid,text,text),
  dal_security.finish_recovery(uuid,text),
  dal_security.clear_recovery_codes(uuid) TO dal_api;

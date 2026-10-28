-- Owner-only migration. No passwords are embedded; provision runtime credentials
-- separately. These roles must never inherit the owner or neon_superuser.
DO $roles$
BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='dal_api') THEN
    CREATE ROLE dal_api LOGIN NOINHERIT NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION;
  END IF;
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='dal_worker') THEN
    CREATE ROLE dal_worker LOGIN NOINHERIT NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION;
  END IF;
  IF EXISTS (SELECT FROM pg_roles r WHERE r.rolname IN ('dal_api','dal_worker') AND
    (r.rolsuper OR r.rolbypassrls OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolinherit
     OR EXISTS (SELECT FROM pg_auth_members m WHERE m.member=r.oid))) THEN
    RAISE EXCEPTION 'Unsafe existing DAL runtime roles; owner must reconcile privileges';
  END IF;
  EXECUTE format('REVOKE CREATE, TEMP ON DATABASE %I FROM PUBLIC', current_database());
  EXECUTE format('GRANT CONNECT ON DATABASE %I TO dal_api, dal_worker', current_database());
END $roles$;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
CREATE SCHEMA dal_security;
REVOKE ALL ON SCHEMA dal_security FROM PUBLIC;
GRANT USAGE ON SCHEMA public, dal_security TO dal_api, dal_worker;
ALTER DEFAULT PRIVILEGES IN SCHEMA dal_security REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

-- A request context is a defense against missing predicates, not arbitrary SQL
-- execution. Only verified Rust authentication binds it, using SET LOCAL.
CREATE FUNCTION dal_security.actor() RETURNS uuid LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp AS $$
  SELECT CASE WHEN current_setting('dal.actor',true) ~ '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$'
    THEN current_setting('dal.actor',true)::uuid END
$$;
CREATE FUNCTION dal_security.credential() RETURNS uuid LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp AS $$
  SELECT CASE WHEN current_setting('dal.api_token',true) ~ '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$'
    THEN current_setting('dal.api_token',true)::uuid END
$$;
CREATE FUNCTION dal_security.session() RETURNS boolean LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp AS $$
  SELECT coalesce(current_setting('dal.auth_kind',true)='session',false)
$$;
CREATE FUNCTION dal_security.allowed(required_scope text DEFAULT NULL) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT EXISTS (SELECT FROM public.users u WHERE u.id=dal_security.actor() AND u.email_verified AND
    (dal_security.session() OR (current_setting('dal.auth_kind',true)='api' AND EXISTS
      (SELECT FROM public.api_tokens t WHERE t.id=dal_security.credential() AND t.user_id=u.id
       AND (t.expires_at IS NULL OR t.expires_at>now())
       AND (required_scope IS NULL OR required_scope=ANY(t.scopes))))))
$$;
CREATE FUNCTION dal_security.admin() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT dal_security.session() AND dal_security.allowed() AND EXISTS
    (SELECT FROM public.users WHERE id=dal_security.actor() AND is_admin)
$$;
CREATE FUNCTION dal_security.member(package uuid, owner_only boolean DEFAULT false) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT dal_security.admin() OR EXISTS (SELECT FROM public.package_owners po
    WHERE po.package_id=package AND po.user_id=dal_security.actor() AND (NOT owner_only OR po.role='owner'))
$$;
CREATE FUNCTION dal_security.valid_invite(package uuid, target uuid, target_role text, inviter uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT dal_security.session() AND dal_security.allowed() AND target=dal_security.actor() AND EXISTS
    (SELECT FROM public.owner_invites i WHERE i.package_id=package AND i.invitee_id=target
      AND i.role=target_role AND i.inviter_id=inviter AND i.accepted_at IS NULL
      AND i.declined_at IS NULL AND i.expires_at>now() AND
      (EXISTS (SELECT FROM public.package_owners po WHERE po.package_id=package AND po.user_id=inviter AND po.role='owner')
       OR EXISTS (SELECT FROM public.users u WHERE u.id=inviter AND u.is_admin AND u.email_verified)))
$$;
ALTER TABLE public.packages ADD COLUMN created_by uuid REFERENCES public.users(id) ON DELETE SET NULL;
ALTER TABLE public.packages ALTER COLUMN created_by SET DEFAULT dal_security.actor();
-- Identifies only a package created in this transaction, never an orphaned or
-- old package belonging to the same creator. Runtime cannot update this column.
ALTER TABLE public.packages ADD COLUMN created_in_xact xid8 NOT NULL DEFAULT pg_current_xact_id();

-- Public profiles deliberately omit email, subject, admin flags and revocation.
CREATE VIEW public.dal_public_users WITH (security_barrier=true) AS
  SELECT id,username,display_name,avatar_url,bio,website,created_at FROM public.users;
GRANT SELECT ON public.dal_public_users TO dal_api;

-- Fixed, parameterized service operations bridge pre-authentication workflows.
-- They grant neither arbitrary SQL nor bulk credential/MFA reads.
CREATE FUNCTION dal_security.auth_user(field text, value text) RETURNS SETOF public.users
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT * FROM public.users WHERE
    (field='username' AND lower(username)=lower(value)) OR
    (field='email' AND lower(email)=lower(value)) OR
    (field='sub' AND cognito_sub=value) OR (field='id' AND id::text=value) LIMIT 1
$$;
CREATE FUNCTION dal_security.register_user(uid uuid, uname text, mail text, subject text, display text)
RETURNS SETOF public.users LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  INSERT INTO public.users(id,username,email,cognito_sub,display_name)
    VALUES(uid,uname,mail,subject,display) RETURNING *
$$;
CREATE FUNCTION dal_security.issue_verification(uid uuid, hash text, token_kind text, expiry timestamptz)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
  IF length(hash)<>64 OR token_kind NOT IN ('email_verify','password_reset') OR expiry<=now()
    OR expiry>now()+interval '24 hours' THEN RAISE EXCEPTION 'Invalid verification capability'; END IF;
  INSERT INTO public.verification_tokens(user_id,token_hash,kind,expires_at) VALUES(uid,hash,token_kind,expiry)
    ON CONFLICT(user_id,kind) DO UPDATE SET token_hash=EXCLUDED.token_hash,
      expires_at=EXCLUDED.expires_at,used_at=NULL,created_at=now();
END $$;
CREATE FUNCTION dal_security.consume_verification(hash text, token_kind text) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE uid uuid;
BEGIN
  UPDATE public.verification_tokens SET used_at=now() WHERE token_hash=hash AND kind=token_kind
    AND used_at IS NULL AND expires_at>now() RETURNING user_id INTO uid;
  IF uid IS NOT NULL AND token_kind='email_verify' THEN
    UPDATE public.users SET email_verified=true,updated_at=now() WHERE id=uid;
  END IF;
  RETURN uid;
END $$;
CREATE FUNCTION dal_security.auth_token(hash text) RETURNS SETOF public.api_tokens
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT * FROM public.api_tokens WHERE token_hash=hash AND (expires_at IS NULL OR expires_at>now()) LIMIT 1
$$;
CREATE FUNCTION dal_security.touch_token(hash text) RETURNS void
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  UPDATE public.api_tokens SET last_used_at=now() WHERE token_hash=hash AND (expires_at IS NULL OR expires_at>now())
$$;
CREATE FUNCTION dal_security.mfa_required(uid uuid) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT coalesce((SELECT required FROM public.user_mfa WHERE user_id=uid),false)
$$;
CREATE FUNCTION dal_security.mfa_authorized(uid uuid, hash text) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT EXISTS(SELECT FROM public.mfa_id_tokens t JOIN public.mfa_sessions s USING(refresh_hash)
    WHERE s.user_id=uid AND t.token_hash=hash AND s.expires_at>now() AND t.expires_at>now())
$$;
CREATE FUNCTION dal_security.account_download(vid uuid, pid uuid, ip text, agent text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
  UPDATE public.package_versions SET downloads=downloads+1 WHERE id=vid AND package_id=pid;
  IF NOT FOUND THEN RAISE EXCEPTION 'Download version/package mismatch'; END IF;
  UPDATE public.packages SET downloads=downloads+1,updated_at=now() WHERE id=pid;
  INSERT INTO public.download_logs(version_id,ip_hash,user_agent) VALUES(vid,ip,agent);
END $$;
CREATE FUNCTION dal_security.download_chart(vid uuid, days bigint)
RETURNS TABLE(day date,cnt bigint) LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT created_at::date,count(*) FROM public.download_logs WHERE version_id=vid
    AND created_at>now()-(days::text || ' days')::interval
    GROUP BY created_at::date ORDER BY created_at::date
$$;
CREATE FUNCTION dal_security.lock_stale_user(uid uuid, cutoff timestamptz) RETURNS SETOF public.users
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
  SELECT * FROM public.users WHERE id=uid AND NOT email_verified
    AND created_at<least(cutoff,now()-interval '1 day') FOR UPDATE
$$;


CREATE FUNCTION dal_security.lock_package(package uuid) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE found_id uuid;
BEGIN
  IF NOT ((dal_security.member(package,true) AND dal_security.allowed('owner')) OR
    (dal_security.session() AND dal_security.allowed() AND EXISTS
      (SELECT FROM public.owner_invites i WHERE i.package_id=package AND i.invitee_id=dal_security.actor()
        AND i.accepted_at IS NULL AND i.declined_at IS NULL AND i.expires_at>now()))) THEN
    RAISE EXCEPTION 'Package lock requires verified ownership or invitation';
  END IF;
  SELECT id INTO STRICT found_id FROM public.packages WHERE id=package FOR UPDATE;
  RETURN found_id;
END $$;

-- Explicit API grants. No credential-table UPDATE, no account privilege edits,
-- no schema writes, no TRUNCATE/REFERENCES/TRIGGER and no worker credential access.
REVOKE ALL ON ALL TABLES IN SCHEMA public FROM PUBLIC,dal_api,dal_worker;
GRANT SELECT ON public.dal_public_users,public.users,public.api_tokens,public.packages,
  public.package_versions,public.package_owners,public.owner_invites,public.user_mfa,
  public.mfa_sessions,public.mfa_id_tokens,public.audit_log TO dal_api;
GRANT INSERT,DELETE ON public.api_tokens TO dal_api;
GRANT UPDATE(display_name,bio,website,avatar_url,updated_at,session_invalid_before) ON public.users TO dal_api;
GRANT INSERT ON public.packages,public.package_versions TO dal_api;
GRANT UPDATE(description,repository,homepage,license,readme,keywords,categories,updated_at) ON public.packages TO dal_api;
GRANT DELETE ON public.packages TO dal_api;
GRANT UPDATE(yanked,yank_reason) ON public.package_versions TO dal_api;
GRANT INSERT,UPDATE,DELETE ON public.package_owners,public.owner_invites,public.user_mfa,public.mfa_sessions,public.mfa_id_tokens TO dal_api;
GRANT INSERT ON public.audit_log TO dal_api;
GRANT SELECT,DELETE ON public.users TO dal_worker;

ALTER TABLE public.users ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.users FORCE ROW LEVEL SECURITY;
CREATE POLICY user_read ON public.users FOR SELECT TO dal_api USING
  (id=dal_security.actor() AND dal_security.allowed() OR dal_security.admin());
CREATE POLICY user_update ON public.users FOR UPDATE TO dal_api USING
  (id=dal_security.actor() AND dal_security.allowed('user:write')) WITH CHECK (id=dal_security.actor() AND dal_security.allowed('user:write'));
CREATE POLICY stale_user_read ON public.users FOR SELECT TO dal_worker USING (NOT email_verified AND created_at<now()-interval '1 day');
CREATE POLICY stale_user_delete ON public.users FOR DELETE TO dal_worker USING (NOT email_verified AND created_at<now()-interval '1 day');

ALTER TABLE public.verification_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.verification_tokens FORCE ROW LEVEL SECURITY;
-- No direct runtime policies/grants: only issue/consume capabilities above.
ALTER TABLE public.api_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.api_tokens FORCE ROW LEVEL SECURITY;
CREATE POLICY token_read ON public.api_tokens FOR SELECT TO dal_api USING
  (user_id=dal_security.actor() AND dal_security.session() AND dal_security.allowed());
CREATE POLICY token_insert ON public.api_tokens FOR INSERT TO dal_api WITH CHECK
  (user_id=dal_security.actor() AND dal_security.session() AND dal_security.allowed());
CREATE POLICY token_delete ON public.api_tokens FOR DELETE TO dal_api USING
  (user_id=dal_security.actor() AND dal_security.session() AND dal_security.allowed());

ALTER TABLE public.packages ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.packages FORCE ROW LEVEL SECURITY;
CREATE POLICY package_read ON public.packages FOR SELECT TO dal_api USING (true);
CREATE POLICY package_insert ON public.packages FOR INSERT TO dal_api WITH CHECK
  (created_by=dal_security.actor() AND created_in_xact=pg_current_xact_id() AND dal_security.allowed('publish:new'));
CREATE POLICY package_update ON public.packages FOR UPDATE TO dal_api USING
  (dal_security.member(id) AND (dal_security.allowed('publish:update') OR dal_security.allowed('owner')
   OR (created_by=dal_security.actor() AND created_in_xact=pg_current_xact_id() AND dal_security.allowed('publish:new')))) WITH CHECK
  (dal_security.member(id) AND dal_security.allowed());
CREATE POLICY package_delete ON public.packages FOR DELETE TO dal_api USING (dal_security.admin());

ALTER TABLE public.package_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.package_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY version_read ON public.package_versions FOR SELECT TO dal_api USING (true);
CREATE POLICY version_insert ON public.package_versions FOR INSERT TO dal_api WITH CHECK
  (published_by=dal_security.actor() AND dal_security.member(package_id) AND
    (dal_security.allowed('publish:update') OR (dal_security.allowed('publish:new') AND EXISTS
      (SELECT FROM public.packages WHERE id=package_id AND created_by=dal_security.actor() AND created_in_xact=pg_current_xact_id()))));
CREATE POLICY version_yank ON public.package_versions FOR UPDATE TO dal_api USING
  (dal_security.member(package_id) AND dal_security.allowed('yank')) WITH CHECK
  (dal_security.member(package_id) AND dal_security.allowed('yank'));

ALTER TABLE public.package_owners ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.package_owners FORCE ROW LEVEL SECURITY;
CREATE POLICY owner_read ON public.package_owners FOR SELECT TO dal_api USING (true);
CREATE POLICY owner_insert ON public.package_owners FOR INSERT TO dal_api WITH CHECK
  ((dal_security.member(package_id,true) AND dal_security.allowed('owner')) OR
   (user_id=dal_security.actor() AND role='owner' AND dal_security.allowed('publish:new')
    AND EXISTS (SELECT FROM public.packages WHERE id=package_id AND created_by=dal_security.actor() AND created_in_xact=pg_current_xact_id())
    AND NOT EXISTS (SELECT FROM public.package_owners po WHERE po.package_id=package_owners.package_id)) OR
   dal_security.valid_invite(package_id,user_id,role,invited_by));
CREATE POLICY owner_update ON public.package_owners FOR UPDATE TO dal_api USING
  ((dal_security.member(package_id,true) AND dal_security.allowed('owner')) OR
   dal_security.valid_invite(package_id,user_id,role,invited_by)) WITH CHECK
  ((dal_security.member(package_id,true) AND dal_security.allowed('owner')) OR
   dal_security.valid_invite(package_id,user_id,role,invited_by));
CREATE POLICY owner_delete ON public.package_owners FOR DELETE TO dal_api USING
  (dal_security.member(package_id,true) AND dal_security.allowed('owner'));

ALTER TABLE public.owner_invites ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.owner_invites FORCE ROW LEVEL SECURITY;
CREATE POLICY invite_read ON public.owner_invites FOR SELECT TO dal_api USING
  (dal_security.allowed() AND (invitee_id=dal_security.actor() AND dal_security.session()
    OR dal_security.member(package_id,true) AND dal_security.allowed('owner')));
CREATE POLICY invite_insert ON public.owner_invites FOR INSERT TO dal_api WITH CHECK
  (inviter_id=dal_security.actor() AND dal_security.member(package_id,true) AND dal_security.allowed('owner'));
CREATE POLICY invite_update ON public.owner_invites FOR UPDATE TO dal_api USING
  (dal_security.allowed() AND (invitee_id=dal_security.actor() AND dal_security.session()
    OR dal_security.member(package_id,true) AND dal_security.allowed('owner'))) WITH CHECK
  (dal_security.allowed() AND (invitee_id=dal_security.actor() AND dal_security.session()
    OR dal_security.member(package_id,true) AND dal_security.allowed('owner')));
CREATE POLICY invite_delete ON public.owner_invites FOR DELETE TO dal_api USING
  (dal_security.member(package_id,true) AND dal_security.allowed('owner'));


-- Invitees may accept/decline, but never rewrite the capability they received.
CREATE FUNCTION dal_security.protect_invite() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
  IF current_user=pg_get_userbyid((SELECT relowner FROM pg_class WHERE oid='public.owner_invites'::regclass)) THEN RETURN NEW; END IF;
  IF ROW(NEW.package_id,NEW.invitee_id,NEW.inviter_id,NEW.token_hash,NEW.role,NEW.expires_at,NEW.created_at)
     IS DISTINCT FROM ROW(OLD.package_id,OLD.invitee_id,OLD.inviter_id,OLD.token_hash,OLD.role,OLD.expires_at,OLD.created_at)
     AND NOT (dal_security.member(OLD.package_id,true) AND dal_security.allowed('owner')) THEN
    RAISE EXCEPTION 'Only an authorized package owner can change an invitation';
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER owner_invite_integrity BEFORE UPDATE ON public.owner_invites
FOR EACH ROW EXECUTE FUNCTION dal_security.protect_invite();

ALTER TABLE public.user_mfa ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.user_mfa FORCE ROW LEVEL SECURITY;
CREATE POLICY mfa_guard ON public.user_mfa TO dal_api USING
  (user_id=dal_security.actor() AND dal_security.session() AND dal_security.allowed()) WITH CHECK
  (user_id=dal_security.actor() AND dal_security.session() AND dal_security.allowed());
ALTER TABLE public.mfa_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.mfa_sessions FORCE ROW LEVEL SECURITY;
CREATE POLICY mfa_session ON public.mfa_sessions TO dal_api USING
  (user_id=dal_security.actor() AND dal_security.session() AND dal_security.allowed()) WITH CHECK
  (user_id=dal_security.actor() AND dal_security.session() AND dal_security.allowed());
ALTER TABLE public.mfa_id_tokens ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.mfa_id_tokens FORCE ROW LEVEL SECURITY;
CREATE POLICY mfa_id_token ON public.mfa_id_tokens TO dal_api USING
  (dal_security.session() AND dal_security.allowed() AND EXISTS
    (SELECT FROM public.mfa_sessions s WHERE s.refresh_hash=mfa_id_tokens.refresh_hash AND s.user_id=dal_security.actor())) WITH CHECK
  (dal_security.session() AND dal_security.allowed() AND EXISTS
    (SELECT FROM public.mfa_sessions s WHERE s.refresh_hash=mfa_id_tokens.refresh_hash AND s.user_id=dal_security.actor()));
ALTER TABLE public.download_logs ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.download_logs FORCE ROW LEVEL SECURITY;
-- Only aggregate/download-accounting service functions expose this private telemetry.
ALTER TABLE public.audit_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.audit_log FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_read ON public.audit_log FOR SELECT TO dal_api USING (dal_security.admin());
CREATE POLICY audit_append ON public.audit_log FOR INSERT TO dal_api WITH CHECK
  (actor_id=dal_security.actor() AND dal_security.allowed());

REVOKE ALL ON ALL FUNCTIONS IN SCHEMA dal_security FROM PUBLIC;
GRANT EXECUTE ON ALL FUNCTIONS IN SCHEMA dal_security TO dal_api;
REVOKE EXECUTE ON FUNCTION dal_security.lock_stale_user(uuid,timestamptz) FROM dal_api;
GRANT EXECUTE ON FUNCTION dal_security.lock_stale_user(uuid,timestamptz) TO dal_worker;

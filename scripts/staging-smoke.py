"""Opt-in dedicated STAGING checks. Default is a credential-free plan, never execution."""
import argparse
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]


def validate(config, approved):
    if not approved or config.get("environment") != "staging" or config.get("production") is not False:
        raise ValueError("Execution requires explicit staging-only approval and reviewed nonproduction configuration")
    api = config.get("api", "")
    if api:
        url = urllib.parse.urlsplit(api)
        if url.scheme != "https" or (url.hostname or "").rstrip(".").lower() in {"api.dal.fidan.dev", "dal.fidan.dev"} or not url.hostname or url.username or url.password or url.path not in {"", "/"} or url.port not in {None, 443} or url.query or url.fragment:
            raise ValueError("Use only the reviewed dedicated staging API")
    bucket = config.get("bucket", "")
    if bucket and ("prod" in bucket.lower() or bucket.lower() == "fidan-releases"):
        raise ValueError("Production/release buckets are forbidden")


def command(arguments, env=None):
    result = subprocess.run(arguments, env=env, text=True, capture_output=True, timeout=180)
    if result.returncode:
        raise RuntimeError(f"{Path(arguments[0]).name} failed ({result.returncode}); provider details intentionally not logged")
    return result.stdout.strip()


def request(url, *, token=None, body=None, headers=None):
    fields = dict(headers or {})
    if token:
        fields["Authorization"] = "Bearer " + token
    data = None if body is None else json.dumps(body).encode()
    if data is not None:
        fields["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=fields)
    # Do not allow a redirected API request to forward credentials.
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, hdrs, newurl):
            return None
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=30) as response:
            return response.status, response.read(1024 * 1024 + 1), response.headers
    except urllib.error.HTTPError as error:
        return error.code, error.read(4096), error.headers
    except Exception:
        raise RuntimeError("Staging HTTP request failed; credential URLs are intentionally omitted") from None


def aws(config):
    env = dict(os.environ)
    for key in list(env):
        if key.startswith(("AWS_ACCESS_KEY", "AWS_SECRET", "AWS_SESSION", "AWS_WEB_IDENTITY", "AWS_CONTAINER_CREDENTIALS", "AWS_ROLE_ARN")):
            env.pop(key)
    env["AWS_EC2_METADATA_DISABLED"] = "true"
    profile = config["aws_profile"]
    if not profile or profile.startswith("REPLACE"):
        raise ValueError("An isolated staging credential profile is required")
    return ["aws", "--profile", profile, "--region", config["region"]], env


def database_identity(url):
    """Credential-free identity; reject libpq routing overrides before tool execution."""
    parsed = urllib.parse.urlsplit(url)
    allowed_parameters = {"sslmode", "sslrootcert", "sslcert", "sslkey", "connect_timeout", "application_name"}
    parameters = urllib.parse.parse_qsl(parsed.query, strict_parsing=True)
    if (parsed.scheme not in {"postgres", "postgresql"} or not parsed.hostname
            or parsed.fragment or "," in parsed.hostname
            or any(key not in allowed_parameters for key, _ in parameters)):
        raise ValueError("A single-host PostgreSQL URL without routing overrides is required")
    host = parsed.hostname.rstrip(".").lower()
    name = urllib.parse.unquote(parsed.path.removeprefix("/"), errors="strict")
    if not name or "/" in name or any(ord(character) < 32 for character in host + name):
        raise ValueError("An explicit database identity is required")
    authority = f"[{host}]" if ":" in host else host
    identity = f"postgresql://{authority}:{parsed.port or 5432}/{urllib.parse.quote(name, safe='')}"
    return identity, name


def database(config):
    source = os.environ["DAL_STAGING_DATABASE_URL"]
    target = os.environ["DAL_STAGING_RESTORE_URL"]
    source_identity, _ = database_identity(source)
    target_identity, target_name = database_identity(target)
    # These confirmations must come from an independently reviewed operator
    # inventory, never from the untrusted smoke-test JSON or URL's hostname.
    if (os.environ.get("DAL_CONFIRMED_STAGING_SOURCE") != source_identity
            or os.environ.get("DAL_CONFIRMED_STAGING_TARGET") != target_identity):
        raise ValueError("Both database identities require separate nonproduction confirmation")
    if source_identity == target_identity or not target_name.startswith("dal_audit_restore_"):
        raise ValueError("Restore target must be a separate owner-provisioned dal_audit_restore_ database")
    # libpq environment defaults must not redirect an otherwise approved URL.
    env = {key: value for key, value in os.environ.items()
           if key not in {"PGHOST", "PGHOSTADDR", "PGPORT", "PGDATABASE", "PGSERVICE", "PGSERVICEFILE", "PGSYSCONFDIR", "PGOPTIONS"}}
    query = lambda url, sql: command(["psql", "-X", "-A", "-t", url, "-v", "ON_ERROR_STOP=1", "-c", sql], env)
    if query(target, "SELECT count(*) FROM information_schema.tables WHERE table_schema='public'") != "0":
        raise ValueError("Restore target is not empty; nothing will be overwritten")
    migrations = query(source, "SELECT count(*) FROM _sqlx_migrations WHERE success")
    directory = ROOT / "target/staging-private"
    directory.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="restore-", dir=directory) as temp:
        dump = Path(temp) / "staging.dump"
        command(["pg_dump", source, "-Fc", "-f", str(dump)], env)
        dump.chmod(0o600)
        command(["pg_restore", "--exit-on-error", "--no-owner", "--no-acl", "--dbname", target, str(dump)], env)
    if query(target, "SELECT count(*) FROM _sqlx_migrations WHERE success") != migrations:
        raise RuntimeError("Restored migration journal differs")


def r2(config):
    args, env = aws(config)
    args += ["--endpoint-url", config["r2_endpoint"]]
    endpoint = urllib.parse.urlsplit(config["r2_endpoint"])
    if endpoint.scheme != "https" or not (endpoint.hostname or "").endswith(".r2.cloudflarestorage.com") or endpoint.username or endpoint.password or endpoint.path not in {"", "/"} or endpoint.query or endpoint.fragment:
        raise ValueError("Expected the reviewed staging R2 S3 endpoint")
    bucket = config["bucket"]
    if not bucket or bucket.startswith("REPLACE"):
        raise ValueError("Explicit staging bucket required")
    key = "audit-smoke/" + str(uuid.uuid4())
    with tempfile.TemporaryDirectory() as temp:
        file = Path(temp) / "fixture.txt"
        file.write_bytes(b"DAL staging fixture")
        uploaded = False
        try:
            command(args + ["s3api", "put-object", "--bucket", bucket, "--key", key, "--body", str(file)], env)
            uploaded = True
            url = command(args + ["s3", "presign", f"s3://{bucket}/{key}", "--expires-in", "10"], env)
            status, body, _ = request(url)
            if status != 200 or body != file.read_bytes():
                raise RuntimeError("Staging signed GET/checksum mismatch")
            if config.get("cors_origin"):
                status, _, headers = request(url, headers={"Origin": config["cors_origin"]})
                if status != 200 or headers.get("Access-Control-Allow-Origin") != config["cors_origin"]:
                    raise RuntimeError("Staging CORS policy mismatch")
            time.sleep(12)
            status, _, _ = request(url)
            if status not in {400, 403}:
                raise RuntimeError("Provider did not reject expired signed GET")
        finally:
            if uploaded:
                command(args + ["s3api", "delete-object", "--bucket", bucket, "--key", key], env)


def auth(config):
    api = config["api"].rstrip("/")
    status, _, _ = request(api + "/auth/me", token=os.environ["DAL_STAGING_SESSION_TOKEN"])
    if status != 200:
        raise RuntimeError("Staging JWT session rejected")
    status, _, _ = request(api + "/tokens", token=os.environ["DAL_STAGING_SCOPED_TOKEN"])
    if status != 403:
        raise RuntimeError("Scoped credential was not denied session-only token administration")


def queue(config):
    args, env = aws(config)
    if config.get("queue_isolation") != "dedicated_no_worker":
        raise ValueError("Queue transport smoke requires a dedicated staging FIFO queue with no worker")
    identity = json.loads(command(args + ["sts", "get-caller-identity"], env))
    if identity["Account"] != config["expected_aws_account"]:
        raise ValueError("Staging AWS account mismatch")
    url = config["queue_url"]
    parsed = urllib.parse.urlsplit(url)
    parts = parsed.path.strip("/").split("/")
    if (parsed.scheme != "https" or parsed.hostname != f"sqs.{config['region']}.amazonaws.com"
            or parsed.username or parsed.password or parsed.query or parsed.fragment
            or len(parts) != 2 or not parts[1].startswith("dal-audit-staging-")
            or not parts[1].endswith(".fifo") or parts[0] != identity["Account"]):
        raise ValueError("Queue belongs to another account")
    nonce = str(uuid.uuid4())
    command(args + ["sqs", "send-message", "--queue-url", url, "--message-body", nonce, "--message-group-id", "audit-smoke", "--message-deduplication-id", nonce], env)
    value = json.loads(command(args + ["sqs", "receive-message", "--queue-url", url, "--wait-time-seconds", "10", "--max-number-of-messages", "1"], env))
    message = value.get("Messages", [{}])[0]
    if message.get("Body") != nonce:
        raise RuntimeError("Staging queue roundtrip failed; unrelated messages were not deleted")
    command(args + ["sqs", "delete-message", "--queue-url", url, "--receipt-handle", message["ReceiptHandle"]], env)


def mail(config):
    credentials = os.environ["DAL_STAGING_MAILJET_KEY"] + ":" + os.environ["DAL_STAGING_MAILJET_SECRET"]
    status, body, _ = request("https://api.mailjet.com/v3.1/send", headers={"Authorization": "Basic " + base64.b64encode(credentials.encode()).decode()}, body={"SandboxMode": True, "Messages": [{"From": {"Email": config["mail_from"]}, "To": [{"Email": config["mail_to"]}], "Subject": "DAL staging sandbox", "TextPart": "Validation only; sandbox delivery disabled"}]})
    if status != 200 or json.loads(body)["Messages"][0]["Status"] != "success":
        raise RuntimeError("Mailjet sandbox validation failed; no provider payload logged")


OPERATIONS = {"database": database, "r2": r2, "auth": auth, "queue": queue, "mail": mail}
if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=OPERATIONS)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--approve-staging-only", action="store_true")
    options = parser.parse_args()
    if not options.execute:
        print(f"PLAN ONLY: {options.mode}; no credential lookup or network operation")
    else:
        try:
            config = json.loads(options.config.read_text())
            validate(config, options.approve_staging_only)
            OPERATIONS[options.mode](config)
            print(f"STAGING CHECK COMPLETE: {options.mode}; production approval is separate")
        except Exception as failure:
            # Never emit exception values: tools/env errors can contain credential URLs.
            raise SystemExit(f"Staging check failed ({type(failure).__name__}); no success is claimed") from None

import { randomUUID, createHash } from "node:crypto";
// Local OCI smoke gate: dedicated services, fake credentials, no host secret mounts.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
mkdirSync(path.join(root, "target"), { recursive: true });
const scratch = mkdtempSync(path.join(root, "target", "image-test-"));
const project = `dal-image-audit-${process.pid}-${randomUUID().slice(0, 8)}`;
const compose = ["compose", "-p", project, "-f", "docker-compose.test.yml"];
const owned = [];
function checkLocalRestore() {
  // Only this runner's disposable Compose database; no host DATABASE_URL is read.
  const exec = [...compose, "exec", "-T", "postgres-test"];
  const sql = (database, statement, allowed = [0]) => docker([
    ...exec, "psql", "-X", "-A", "-t", "-U", "dal_test", "-d", database,
    "-v", "ON_ERROR_STOP=1", "-c", statement,
  ], allowed);
  sql("dal_test", `
    INSERT INTO users (username,email,cognito_sub,email_verified,session_invalid_before)
      VALUES ('restore-audit','restore@example.test','restore-fixture',true,now());
    INSERT INTO packages (name,license) VALUES ('restore-audit','Apache-2.0');
    INSERT INTO package_owners (package_id,user_id)
      SELECT p.id,u.id FROM packages p CROSS JOIN users u;
    INSERT INTO api_tokens (user_id,name,token_hash,prefix,scopes)
      SELECT id,'restore-fixture','not-a-real-verifier','fixture',ARRAY['yank'] FROM users;
    INSERT INTO package_versions (package_id,version,checksum,size_bytes,s3_key,published_by,yanked)
      SELECT p.id,'0.1.0',repeat('a',64),1,'packages/restore-audit/fixture.tar.gz',u.id,true
      FROM packages p CROSS JOIN users u;
  `);
  const snapshot = `SELECT json_build_object(
    'users',(SELECT json_agg(u) FROM users u),
    'packages',(SELECT json_agg(p) FROM packages p),
    'owners',(SELECT json_agg(o) FROM package_owners o),
    'tokens',(SELECT json_agg(t) FROM api_tokens t),
    'versions',(SELECT json_agg(v) FROM package_versions v),
    'migrations',(SELECT json_agg(m ORDER BY version) FROM _sqlx_migrations m)
  )::text`;
  const before = sql("dal_test", snapshot);
  docker([...exec, "pg_dump", "-U", "dal_test", "-d", "dal_test", "-Fc", "-f", "/tmp/dal-audit.dump"]);
  docker([...exec, "createdb", "-U", "dal_test", "dal_restore_test"]);
  docker([...exec, "pg_restore", "--exit-on-error", "--no-owner", "-U", "dal_test", "-d", "dal_restore_test", "/tmp/dal-audit.dump"]);
  assert.deepEqual(JSON.parse(sql("dal_restore_test", snapshot)), JSON.parse(before));
  assert.equal(sql("dal_restore_test", "SELECT count(*) FROM users WHERE session_invalid_before IS NOT NULL"), "1");
  // Restored unique/foreign-key definitions must still enforce real constraints.
  sql("dal_restore_test", `DO $restore$ BEGIN
    BEGIN
      INSERT INTO users(username,email,cognito_sub) VALUES ('RESTORE-AUDIT','other@example.test','other');
      RAISE EXCEPTION 'Restored uniqueness was not enforced';
    EXCEPTION WHEN unique_violation THEN NULL;
    END;
    BEGIN
      INSERT INTO package_owners(package_id,user_id) VALUES(gen_random_uuid(),gen_random_uuid());
      RAISE EXCEPTION 'Restored foreign key was not enforced';
    EXCEPTION WHEN foreign_key_violation THEN NULL;
    END;
  END $restore$`);
  assert.equal(sql("dal_restore_test", "SELECT count(*) FROM users"), "1");
  console.log("Disposable PostgreSQL dump/restore preserved migrations, revocation cutoff, token scopes, ownership and yanked archive/checksum references; constraints enforce uniqueness.");
}

function docker(args, allowed = [0]) {
  const result = spawnSync("docker", args, {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
    timeout: 180000,
  });
  if (result.error) throw result.error;
  assert(
    allowed.includes(result.status),
    `docker ${args[0]} failed (${result.status}): ${result.stderr}`,
  );
  return result.stdout.trim();
}
try {
  const emptyEnv = path.join(scratch, "empty.env");
  writeFileSync(emptyEnv, "");
  const localConfig = JSON.parse(docker([
    "compose", "--env-file", emptyEnv, "-p", project, "-f", "docker-compose.yml",
    "config", "--no-env-resolution", "--format", "json",
  ]));
  for (const [service, config] of Object.entries(localConfig.services)) {
    for (const binding of config.ports ?? []) {
      assert(["127.0.0.1", "::1"].includes(binding.host_ip), `${service} exposes local development services beyond loopback`);
    }
  }
  console.log("Local development port bindings stay on loopback; real environment files were not resolved.");
  docker([...compose, "up", "-d", "--wait", "--quiet-pull"]);
  const environment = {
    AWS_ACCESS_KEY_ID: "test",
    AWS_SECRET_ACCESS_KEY: "test",
    AWS_EC2_METADATA_DISABLED: "true",
    AWS_REGION: "eu-central-1",
    DATABASE_URL: "postgres://dal_test:test@postgres-test:5432/dal_test",
    DAL_STORAGE_ENDPOINT_URL: "http://localstack-test:4566",
    DAL_STORAGE_BUCKET: "dal-test",
    DAL_STORAGE_REGION: "eu-central-1",
    DAL_STORAGE_ACCESS_KEY_ID: "test",
    DAL_STORAGE_SECRET_ACCESS_KEY: "test",
    DAL_SQS_ENDPOINT_URL: "http://localstack-test:4566",
    DAL_SQS_QUEUE_URL:
      "http://localstack-test:4566/000000000000/dal-jobs-test.fifo",
    DAL_COGNITO_USER_POOL_ID: "local-test",
    DAL_COGNITO_CLIENT_ID: "local-test",
    DAL_COGNITO_ENDPOINT_URL: "http://127.0.0.1:9",
    MAILJET_API_KEY: "test",
    MAILJET_SECRET_KEY: "test",
    MAILJET_FROM_EMAIL: "test@example.test",
    MAILJET_BASE_URL: "http://127.0.0.1:9",
    DAL_LISTEN_ADDR: "0.0.0.0:8080",
    DAL_BASE_URL: "http://localhost:4174",
    RUST_LOG: "warn",
  };
  const envFile = path.join(scratch, "local.env");
  writeFileSync(
    envFile,
    Object.entries(environment)
      .map(([key, value]) => `${key}=${value}`)
      .join("\n"),
  );
  const summary = [];
  for (const kind of ["server", "worker"]) {
    const name = `${project}-${kind}`;
    owned.push(name);
    docker([
      "run",
      "-d",
      "--name",
      name,
      "--network",
      `${project}_default`,
      "--read-only",
      "--tmpfs",
      "/tmp:size=16m,mode=1777",
      "--cap-drop",
      "ALL",
      "--security-opt",
      "no-new-privileges:true",
      "--memory",
      "512m",
      "--cpus",
      "1",
      "--env-file",
      envFile,
      process.env[`DAL_${kind.toUpperCase()}_IMAGE`] ?? `dal-${kind}:audit-local`,
    ]);
    assert.equal(docker(["exec", name, "id", "-u"]), "10001");
    for (const packageName of ["curl", "libcurl4t64"]) {
      const installed = docker(["exec", name, "dpkg-query", "-W", "-f=${db:Status-Status}", packageName], [0, 1]);
      assert.notEqual(installed, "installed", `Unneeded vulnerable runtime package: ${packageName}`);
    }
    for (const file of ["LICENSE", "NOTICE"]) {
      const expected = createHash("sha256").update(readFileSync(path.join(root, file))).digest("hex");
      assert.equal(docker(["exec", name, "sha256sum", `/usr/share/doc/dal/${file}`]).split(/\s/)[0], expected);
    }
    const inspection = JSON.parse(docker(["inspect", name]))[0];
    assert(inspection.HostConfig.ReadonlyRootfs);
    assert.equal(inspection.HostConfig.Memory, 512 * 1024 * 1024);
    if (kind === "server") {
      let ready = false;
      for (let i = 0; i < 120; i++) {
        const result = spawnSync(
          "docker",
          [
            "exec",
            name,
            "/usr/local/bin/dal-healthcheck",
            "--ready",
          ],
          { encoding: "utf8", windowsHide: true, timeout: 5000 },
        );
        if (result.status === 0) {
          ready = true;
          break;
        }
        assert.equal(
          JSON.parse(docker(["inspect", name]))[0].State.Running,
          true,
        );
        await new Promise((resolve) => setTimeout(resolve, 500));
      }
      assert(ready, "container readiness failed");
      docker([
        "exec",
        name,
        "/usr/local/bin/dal-healthcheck",
      ]);
      checkLocalRestore();
    } else {
      await new Promise((resolve) => setTimeout(resolve, 2500));
      assert.equal(
        JSON.parse(docker(["inspect", name]))[0].State.Running,
        true,
      );
    }
    const stats = docker([
      "stats",
      "--no-stream",
      "--format",
      "{{json .}}",
      name,
    ]);
    docker(["stop", "--time", "35", name]);
    const stopped = JSON.parse(docker(["inspect", name]))[0];
    assert.equal(stopped.State.ExitCode, 0, `${kind} must shut down cleanly`);
    summary.push({
      kind,
      image: inspection.Image,
      uid: 10001,
      readOnly: true,
      idleStats: JSON.parse(stats),
      exitCode: stopped.State.ExitCode,
    });
  }
  writeFileSync(
    path.join(root, "target/audit-logs/image-smoke-final.json"),
    JSON.stringify(summary, null, 2),
  );
  console.log(
    "Both local images: non-root, read-only, bounded memory, API health/readiness, worker start and graceful shutdown passed.",
  );
} finally {
  for (const name of owned) docker(["rm", "-f", name], [0, 1]);
  docker([...compose, "down", "--volumes"]);
  rmSync(scratch, { recursive: true, force: true });
}

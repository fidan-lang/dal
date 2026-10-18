import { randomUUID } from "node:crypto";
// Local OCI smoke gate: dedicated services, fake credentials, no host secret mounts.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const scratch = mkdtempSync(path.join(root, "target", "image-test-"));
const project = `dal-image-audit-${process.pid}-${randomUUID().slice(0, 8)}`;
const compose = ["compose", "-p", project, "-f", "docker-compose.test.yml"];
const owned = [];
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
      `dal-${kind}:audit-local`,
    ]);
    assert.equal(docker(["exec", name, "id", "-u"]), "10001");
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
            "curl",
            "--fail",
            "--silent",
            "--max-time",
            "2",
            "http://127.0.0.1:8080/readyz",
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
        "curl",
        "--fail",
        "--silent",
        "http://127.0.0.1:8080/health",
      ]);
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
  docker([...compose, "down"]);
  rmSync(scratch, { recursive: true, force: true });
}

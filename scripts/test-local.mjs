import { randomUUID } from "node:crypto";
// Disposable local-only readiness gate. No .env files or cloud credentials are trusted.
import { spawn, spawnSync } from "node:child_process";
import { createServer } from "node:http";
import {
  openSync,
  closeSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { localTestEndpoint } from "../frontend/e2e/local-test-endpoints.mjs";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const flags = new Set(process.argv.slice(2));
for (const flag of flags)
  if (!["--with-cognito", "--e2e", "--reuse-services"].includes(flag))
    throw new Error(`Unknown test option: ${flag}`);
const auth = flags.has("--with-cognito") || flags.has("--e2e");
const logs = path.join(root, "target", "audit-logs");
mkdirSync(logs, { recursive: true });
const scratch = mkdtempSync(path.join(logs, "local-gate-"));
writeFileSync(path.join(scratch, "aws-config"), "");
writeFileSync(path.join(scratch, "aws-credentials"), "");
const project = `dal-audit-${process.pid}-${randomUUID().slice(0, 8)}`;
const compose = [
  "compose",
  "-p",
  project,
  "-f",
  path.join(root, "docker-compose.test.yml"),
];
const children = [];
const env = {
  ...process.env,
  AWS_CONFIG_FILE: path.join(scratch, "aws-config"),
  AWS_SHARED_CREDENTIALS_FILE: path.join(scratch, "aws-credentials"),
  AWS_WEB_IDENTITY_TOKEN_FILE: "",
  AWS_ROLE_ARN: "",
  AWS_DEFAULT_PROFILE: "",
  CLOUDFLARE_API_TOKEN: "",
  CLOUDFLARE_API_KEY: "",
  CF_API_TOKEN: "",
  CLOUDFLARE_ACCOUNT_ID: "",
  AWS_ACCESS_KEY_ID: "test",
  AWS_SECRET_ACCESS_KEY: "test",
  AWS_SESSION_TOKEN: "",
  AWS_PROFILE: "",
  AWS_EC2_METADATA_DISABLED: "true",
  AWS_REGION: "eu-central-1",
  AWS_CONTAINER_CREDENTIALS_FULL_URI: "",
  AWS_CONTAINER_CREDENTIALS_RELATIVE_URI: "",
  DATABASE_URL: "postgres://dal_test:test@127.0.0.1:5433/dal_test",
  TEST_DATABASE_URL: "postgres://dal_test:test@127.0.0.1:5433/dal_test",
  TEST_STORAGE_ENDPOINT_URL: "http://127.0.0.1:4567",
  TEST_SQS_ENDPOINT_URL: "http://127.0.0.1:4567",
  TEST_SQS_QUEUE_URL: "http://127.0.0.1:4567/000000000000/dal-jobs-test.fifo",
  DAL_STORAGE_ENDPOINT_URL: "http://127.0.0.1:4567",
  DAL_STORAGE_BUCKET: "dal-test",
  DAL_STORAGE_REGION: "eu-central-1",
  DAL_STORAGE_ACCESS_KEY_ID: "test",
  DAL_STORAGE_SECRET_ACCESS_KEY: "test",
  DAL_SQS_ENDPOINT_URL: "http://127.0.0.1:4567",
  DAL_SQS_QUEUE_URL: "http://127.0.0.1:4567/000000000000/dal-jobs-test.fifo",
  DAL_COGNITO_ENDPOINT_URL: "http://127.0.0.1:9230",
  TEST_COGNITO_ENDPOINT_URL: "http://127.0.0.1:9230",
  DAL_LISTEN_ADDR: "127.0.0.1:18080",
  DAL_BASE_URL: "http://127.0.0.1:4174",
  DAL_TRUSTED_PROXIES: "",
  MAILJET_API_KEY: "test",
  MAILJET_SECRET_KEY: "test",
  MAILJET_FROM_EMAIL: "test@example.test",
  MAILJET_FROM_NAME: "Dal local tests",
  MAILJET_BASE_URL: "http://127.0.0.1:8025",
  PUBLIC_API_BASE: "http://127.0.0.1:18080",
  PUBLIC_API_URL: "http://127.0.0.1:18080",
  DAL_API_BASE_URL: "http://127.0.0.1:18080",
  DAL_MAIL_SINK_URL: "http://127.0.0.1:8025",
  RUST_LOG: process.env.DAL_TEST_LOG ?? "warn",
  PORT: "9230",
  COGNITO_LOCAL_HOST: "127.0.0.1",
};
for (const name of ["DAL_WEB_BASE_URL"]) delete env[name];
for (const [key, value] of Object.entries(env))
  if (
    /(ENDPOINT_URL|BASE_URL|PUBLIC_API_BASE|PUBLIC_API_URL)$/.test(key) &&
    value &&
    /^(https?:)/.test(value)
  )
    localTestEndpoint(value, key);
function start(command, args, cwd, name) {
  const fd = openSync(path.join(logs, `${name}.log`), "w");
  // npm.cmd needs a command host on Windows; command strings here are fixed constants.
  const child =
    process.platform === "win32" && command === "npm"
      ? spawn(
          process.env.ComSpec ?? "cmd.exe",
          ["/d", "/s", "/c", `npm ${args.join(" ")}`],
          { cwd, env, stdio: ["ignore", fd, fd], windowsHide: true },
        )
      : spawn(command, args, {
          cwd,
          env,
          stdio: ["ignore", fd, fd],
          windowsHide: true,
          detached: process.platform !== "win32",
        });
  closeSync(fd);
  children.push(child);
  child.on("error", () => {});
  return child;
}
function waitExit(child, name) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code) =>
      code === 0
        ? resolve()
        : reject(
            new Error(
              `${name} failed (${code}); inspect target/audit-logs/${name}.log locally (may contain test tokens)`,
            ),
          ),
    );
  });
}
async function run(command, args, cwd, name) {
  console.log(`Running ${name}`);
  await waitExit(start(command, args, cwd, name), name);
}
async function ready(url, child) {
  localTestEndpoint(url, "readiness");
  for (let i = 0; i < 120; i++) {
    if (child?.exitCode !== null && child?.exitCode !== undefined)
      throw new Error("Local service exited before readiness");
    try {
      if (
        (
          await fetch(url, {
            redirect: "error",
            signal: AbortSignal.timeout(1000),
          })
        ).ok
      )
        return;
    } catch {}
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`Local service readiness timed out: ${new URL(url).origin}`);
}
let mail;
let composeStarted = false;
try {
  if (!flags.has("--reuse-services")) {
    composeStarted = true;
    await run(
      "docker",
      [...compose, "up", "-d", "--wait", "--quiet-pull"],
      root,
      "local-compose",
    );
  }
  if (auth) {
    mkdirSync(path.join(scratch, ".cognito"));
    const config = JSON.parse(
      readFileSync(path.join(root, ".cognito/config.json"), "utf8"),
    );
    config.ServerConfig.port = 9230;
    config.ServerConfig.hostname = "127.0.0.1";
    config.TokenConfig.IssuerDomain = "http://127.0.0.1:9230";
    writeFileSync(
      path.join(scratch, ".cognito/config.json"),
      JSON.stringify(config),
    );
    await run(
      "npm",
      ["ci"],
      path.join(root, "test"),
      "local-test-dependencies",
    );
    const emulatorPackage = JSON.parse(
      readFileSync(
        path.join(root, "test/node_modules/cognito-local/package.json"),
        "utf8",
      ),
    );
    const executable =
      typeof emulatorPackage.bin === "string"
        ? emulatorPackage.bin
        : emulatorPackage.bin["cognito-local"];
    const emulator = start(
      process.execPath,
      [path.join(root, "test/node_modules/cognito-local", executable)],
      scratch,
      "local-cognito",
    );
    await ready(
      "http://127.0.0.1:9230/local-ready/.well-known/jwks.json",
      emulator,
    );
    await run(
      process.execPath,
      [path.join(root, "scripts/seed-cognito.mjs"), "http://127.0.0.1:9230"],
      scratch,
      "local-cognito-ids",
    );
    const ids = JSON.parse(
      readFileSync(path.join(logs, "local-cognito-ids.log"), "utf8"),
    );
    env.DAL_COGNITO_USER_POOL_ID = env.TEST_COGNITO_POOL_ID = ids.poolId;
    env.DAL_COGNITO_CLIENT_ID = env.TEST_COGNITO_CLIENT_ID = ids.clientId;
  }
  await run(
    "cargo",
    [
      "test",
      "--workspace",
      "--all-targets",
      "--locked",
      "--",
      ...(auth ? ["--include-ignored"] : []),
      "--test-threads=1",
    ],
    root,
    "local-rust-tests",
  );
  if (flags.has("--e2e")) {
    const messages = [];
    mail = createServer(async (req, res) => {
      try {
        if (req.method === "GET" && req.url === "/messages") {
          res.setHeader("Content-Type", "application/json");
          res.end(JSON.stringify(messages));
          return;
        }
        if (req.method === "POST" && req.url === "/reset") {
          messages.length = 0;
          res.end("{}");
          return;
        }
        if (req.method !== "POST" || req.url !== "/send") {
          res.writeHead(404).end();
          return;
        }
        const chunks = [];
        let length = 0;
        for await (const chunk of req) {
          length += chunk.length;
          if (length > 1024 * 1024) {
            res.writeHead(413).end();
            return;
          }
          chunks.push(chunk);
        }
        const payload = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        for (const message of payload.Messages ?? [])
          messages.push({
            body: `${message.TextPart ?? ""}\n${message.HTMLPart ?? ""}`,
          });
        if (messages.length > 1000) messages.splice(0, messages.length - 1000);
        res.setHeader("Content-Type", "application/json");
        res.end('{"Messages":[{"Status":"success"}]}');
      } catch {
        res.writeHead(400).end();
      }
    });
    await new Promise((resolve, reject) => {
      mail.once("error", reject);
      mail.listen(8025, "127.0.0.1", resolve);
    });
    await run(
      "cargo",
      ["build", "--workspace", "--locked"],
      root,
      "local-build",
    );
    const suffix = process.platform === "win32" ? ".exe" : "";
    env.DAL_FIDAN_BIN = path.resolve(
      root,
      "../fidan/target/debug",
      `fidan${suffix}`,
    );
    env.DAL_ARGS_PACKAGE = path.resolve(root, "../args");
    const api = start(
      path.join(root, "target/debug", `dal-server${suffix}`),
      [],
      scratch,
      "local-api",
    );
    start(
      path.join(root, "target/debug", `dal-worker${suffix}`),
      [],
      scratch,
      "local-worker",
    );
    await ready("http://127.0.0.1:18080/readyz", api);
    await run(
      "npm",
      ["run", "e2e"],
      path.join(root, "frontend"),
      "local-browser-e2e",
    );
  }
  console.log(
    "Local readiness gate passed. No production resources were used.",
  );
} finally {
  for (const child of children.reverse())
    if (child.exitCode === null && child.pid) {
      if (process.platform === "win32")
        spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
          stdio: "ignore",
          windowsHide: true,
        });
      else {
        try {
          process.kill(-child.pid, "SIGTERM");
        } catch {}
      }
    }
  if (mail) await new Promise((resolve) => mail.close(resolve));
  if (composeStarted) {
    const result = spawnSync("docker", [...compose, "down"], {
      cwd: root,
      env,
      stdio: "inherit",
      windowsHide: true,
    });
    if (result.status !== 0) process.exitCode = 1;
  }
  rmSync(scratch, { recursive: true, force: true });
}

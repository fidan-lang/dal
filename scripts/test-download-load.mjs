// Called only by the disposable image runner with its local Docker helpers.
import assert from "node:assert/strict";
import { createHash, randomBytes } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import { get } from "node:http";
import { writeFileSync, readFileSync, mkdirSync } from "node:fs";
import path from "node:path";

export async function checkDownloadLoad({ docker, compose, name, scratch, compiler }) {
  const packageRoot = path.join(scratch, "download-load-0.1.0");
  mkdirSync(path.join(packageRoot, "src"), { recursive: true });
  writeFileSync(path.join(packageRoot, "dal.toml"), '[package]\nname = "download-load"\nversion = "0.1.0"\nlicense = "Apache-2.0"\n');
  writeFileSync(path.join(packageRoot, "src/init.fdn"), 'var value = 42\n');
  mkdirSync(path.join(packageRoot, "assets"));
  writeFileSync(path.join(packageRoot, "assets/load.bin"), randomBytes(8 * 1024 * 1024));
  const file = path.join(scratch, "download-load.tar.gz");
  assert.equal(spawnSync("tar", ["-czf", file, "-C", scratch, path.basename(packageRoot)], { windowsHide: true }).status, 0);
  const payload = readFileSync(file);
  const checksum = createHash("sha256").update(payload).digest("hex");
  const manifest = JSON.stringify({ package: { name: "download-load", version: "0.1.0", license: "Apache-2.0" } });
  const storage = docker([...compose, "ps", "-q", "localstack-test"]);
  docker(["cp", file, `${storage}:/tmp/download-load.bin`]);
  docker([...compose, "exec", "-T", "localstack-test", "awslocal", "s3", "cp",
    "/tmp/download-load.bin", "s3://dal-test/packages/download-load/0.1.0.tar.gz"]);
  const sql = statement => docker([...compose, "exec", "-T", "postgres-test", "psql", "-X", "-A", "-t",
    "-U", "dal_test", "-d", "dal_test", "-v", "ON_ERROR_STOP=1", "-c", statement]);
  sql(`
    INSERT INTO users(username,email,cognito_sub,email_verified) VALUES('download-load','load@example.test','download-load',true);
    INSERT INTO packages(name,license) VALUES('download-load','Apache-2.0');
    INSERT INTO package_versions(package_id,version,checksum,size_bytes,s3_key,published_by,manifest)
      SELECT p.id,'0.1.0','${checksum}',${payload.length},'packages/download-load/0.1.0.tar.gz',u.id,'${manifest}'::jsonb
      FROM packages p CROSS JOIN users u WHERE p.name='download-load' AND u.username='download-load';
  `);
  const binding = docker(["port", name, "8080/tcp"]);
  assert.match(binding, /^127\.0\.0\.1:\d+$/);
  const base = `http://${binding}`;
  const route = "/packages/download-load/versions/0.1.0/download";
  const held = [];
  function request(suffix = "", pause = false, headers = {}) {
    return new Promise((resolve, reject) => {
      const req = get(base + route + suffix, { agent: false, headers }, res => {
        res.on("error", reject);
        if (pause && res.statusCode === 200) {
          res.pause();
          held.push({ req, res });
          resolve({ req, res, status: res.statusCode, headers: res.headers });
          return;
        }
        const hash = createHash("sha256");
        let bytes = 0;
        res.on("data", chunk => { bytes += chunk.length; hash.update(chunk); });
        res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, bytes, checksum: hash.digest("hex") }));
      });
      req.on("error", reject);
      req.setTimeout(15_000, () => req.destroy(new Error("Local download test timed out")));
    });
  }
  const stats = () => JSON.parse(docker(["stats", "--no-stream", "--format", "{{json .}}", name]));
  const assertArchive = result => {
    assert.equal(result.status, 200);
    assert.equal(result.bytes, payload.length);
    assert.equal(result.checksum, checksum);
    assert.equal(result.headers.location, undefined);
    assert.match(result.headers["cache-control"], /public.*immutable/);
  };
  async function install(index) {
    const consumer = path.join(scratch, `consumer-${index}`);
    mkdirSync(consumer);
    writeFileSync(path.join(consumer, "dal.toml"), `[package]\nname = "load-consumer-${index}"\nversion = "0.1.0"\n`);
    const env = Object.fromEntries(["PATH", "Path", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"]
      .filter(key => process.env[key]).map(key => [key, process.env[key]]));
    Object.assign(env, { FIDAN_HOME: path.join(consumer, "home"), HOME: consumer, USERPROFILE: consumer,
      LOCALAPPDATA: path.join(consumer, "local"), TMP: scratch, TEMP: scratch });
    await new Promise((resolve, reject) => {
      const child = spawn(compiler, ["dal", "add", "download-load", "--registry", base], {
        cwd: consumer, env, windowsHide: true, signal: AbortSignal.timeout(120_000), stdio: ["ignore", "pipe", "pipe"] });
      let output = "";
      for (const stream of [child.stdout, child.stderr]) stream.on("data", value => { output = (output + value).slice(-16_384); });
      child.once("error", reject);
      child.once("close", status => status === 0 ? resolve() : reject(new Error(`Concurrent local install failed (${status}): ${output}`)));
    });
    assert(readFileSync(path.join(consumer, "dal.lock"), "utf8").includes(checksum));
  }
  const idle = stats();
  try {
    // Eight concurrent anonymous archive fetches behind the same NAT.
    const normal = await Promise.all(Array.from({ length: 8 }, (_, i) => request(`?cache_bust=${i}`)));
    normal.forEach(assertArchive);
    const etag = normal[0].headers.etag;
    assert.equal((await request("", false, { "if-none-match": etag })).status, 304);
    const paused = await Promise.all(Array.from({ length: 16 }, (_, i) => request(`?slow=${i}`, true)));
    assert(paused.every(result => result.status === 200));
    const rejected = await request("?overflow=1", false, { "x-forwarded-for": "192.0.2.99" });
    assert.equal(rejected.status, 429);
    assert(rejected.headers["retry-after"]);
    assert.equal(rejected.headers["cache-control"], "no-store");
    const underLoad = stats();
    for (const { req, res } of paused.slice(0, 8)) { res.destroy(); req.destroy(); }
    assertArchive(await request("?after_abort=1"));
    // The test container uses a five-second deadline; these clients never resume.
    await new Promise(resolve => setTimeout(resolve, 6_000));
    assertArchive(await request("?after_deadline=1"));
    if (compiler) {
      const installs = await Promise.allSettled(Array.from({ length: 8 }, (_, i) => install(i)));
      const failed = installs.filter(result => result.status === "rejected");
      assert.equal(failed.length, 0, failed[0]?.reason?.message);
    }
    const after = stats();
    const repeatedWaves = [];
    for (let wave = 0; wave < 3; wave++) {
      const downloads = await Promise.all(Array.from({ length: 8 }, (_, i) => request(`?wave=${wave}&client=${i}`)));
      downloads.forEach(assertArchive);
      repeatedWaves.push(stats());
    }
    const summary = { archiveBytes: payload.length, concurrentDownloads: 8, pausedDownloads: 16,
      concurrentPackageInstalls: compiler ? 8 : 0, testDeadlineSeconds: 5, idle, underLoad, after, repeatedWaves };
    console.log("Anonymous concurrent downloads, cache-busting, admission, aborts and unpolled-client deadlines passed.");
    return summary;
  } finally {
    for (const { req, res } of held) { res.destroy(); req.destroy(); }
  }
}

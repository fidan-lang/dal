// Print only repository-defined test function names; captured logs may contain JWTs.
import { readFileSync, readdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
function testNames(directory, names = new Set(), sources = new Map(), repositoryRoot = root) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) testNames(file, names, sources, repositoryRoot);
    else if (entry.isFile() && file.endsWith(".rs")) {
      const source = readFileSync(file, "utf8");
      sources.set(path.relative(repositoryRoot, file).replaceAll("\\", "/"), source.split("\n").length);
      for (const match of source.matchAll(/\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/g)) names.add(match[1]);
    }
  }
  return names;
}
export function summarize(log, knownNames, knownSources = new Map(), knownExamples = new Set()) {
  const failed = new Set();
  const plain = log.replace(/\x1b\[[0-9;]*m/g, "");
  for (const match of plain.matchAll(/^test ([a-zA-Z0-9_:]+) \.\.\. FAILED\s*$/gm)) {
    const name = match[1].split("::").at(-1);
    if (knownNames.has(name)) failed.add(name);
  }
  const locations = new Set();
  for (const match of plain.matchAll(/^thread '([a-zA-Z0-9_:]+)'(?: \(\d+\))? panicked at (crates\/[a-zA-Z0-9_./-]+\.rs):(\d+):\d+:$/gm)) {
    const line = Number(match[3]);
    if (knownNames.has(match[1].split("::").at(-1)) && line > 0 && line <= (knownSources.get(match[2]) ?? 0)) {
      locations.add(`Rust panic location: ${match[2]}:${line}`);
    }
  }
  const examples = new Set();
  for (const match of plain.matchAll(/^\[FAIL\] (test[\\/][a-zA-Z0-9_./\\-]+\.fdn) - (compile timed out|compile failed|timed out after [0-9]+(?:\.[0-9]+)? ?s|exited with code -?[0-9]+|wrote to stderr|was expected to fail)\r?$/gm)) {
    const file = match[1].replaceAll("\\", "/");
    if (knownExamples.has(file)) examples.add(`AOT example failed: ${file} (${match[2]})`);
  }
  const categories = new Set();
  for (const match of plain.matchAll(/object upload failed \[(missing_bucket|access_denied|invalid_request|checksum_rejected|authentication_rejected|provider_failure)\]/g)) {
    categories.add(`Storage upload category: ${match[1]}`);
  }
  return [...[...failed].sort().map(name => `Rust test failed: ${name}`), ...locations, ...categories, ...examples].join("\n") ||
    "No recognized test failure name; raw local logs are retained without publishing potentially sensitive output.";
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const log = readFileSync(process.argv[2] ?? path.join(root, "target/audit-logs/local-rust-tests.log"), "utf8");
  const repositoryRoot = path.resolve(process.argv[3] ?? root);
  if (process.argv[4] === "--examples") {
    const tracked = execFileSync("git", ["ls-files", "-z", "--", "test"], { cwd:repositoryRoot, encoding:"utf8" });
    const examples = new Set(tracked.split("\0").filter(file => file.endsWith(".fdn")));
    console.log(summarize(log, new Set(), new Map(), examples));
  } else {
    const sources = new Map();
    console.log(summarize(log, testNames(path.join(repositoryRoot, "crates"), new Set(), sources, repositoryRoot), sources));
  }
}

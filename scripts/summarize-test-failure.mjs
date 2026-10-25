// Print only repository-defined test function names; captured logs may contain JWTs.
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
function testNames(directory, names = new Set(), sources = new Map()) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) testNames(file, names, sources);
    else if (entry.isFile() && file.endsWith(".rs")) {
      const source = readFileSync(file, "utf8");
      sources.set(path.relative(root, file).replaceAll("\\", "/"), source.split("\n").length);
      for (const match of source.matchAll(/\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/g)) names.add(match[1]);
    }
  }
  return names;
}
export function summarize(log, knownNames, knownSources = new Map()) {
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
  return [...[...failed].sort().map(name => `Rust test failed: ${name}`), ...locations].join("\n") ||
    "No recognized test failure name; raw local logs are retained without publishing potentially sensitive output.";
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const log = readFileSync(path.join(root, "target/audit-logs/local-rust-tests.log"), "utf8");
  const sources = new Map();
  console.log(summarize(log, testNames(path.join(root, "crates"), new Set(), sources), sources));
}

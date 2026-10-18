import assert from "node:assert/strict";
import { renderMarkdown, safeWebUrl } from "../src/lib/utils.ts";
const html = renderMarkdown(
  '# Package\n<script>globalThis.__dalInjected=true</script><img src="x" onerror="alert(1)"><a href="jav&#x61;script:alert(1)">unsafe</a>\n\n[Safe](https://fidan.dev)',
);
assert(!/script|onerror|javascript:/i.test(html));
assert(html.includes("<h1>Package</h1>"));
assert(html.includes('href="https://fidan.dev"'));
console.log(
  "Package Markdown: SSR and browser use the same sanitizer; executable content removed.",
);

for (const value of [
  undefined,
  null,
  "",
  "not a URL",
  "javascript:alert(1)",
  "//example.test",
  "https://user:password@example.test",
])
  assert.equal(safeWebUrl(value), null);
assert.equal(
  safeWebUrl("https://example.test/profile").hostname,
  "example.test",
);
console.log("Legacy profile URL rendering is safe and does not throw.");

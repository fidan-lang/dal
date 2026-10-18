/** Reject production endpoints before any test request, browser or credential access.
 * @param {string} value
 * @param {string} label
 */
export function localTestEndpoint(value, label) {
  const url = new URL(value);
  if (
    !["http:", "https:"].includes(url.protocol) ||
    !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname) ||
    url.username ||
    url.password
  ) {
    throw new Error(`${label} requires a credential-free loopback test URL`);
  }
  return value.replace(/\/$/, "");
}

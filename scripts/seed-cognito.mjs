// Only creates disposable emulator state; redirects and non-loopback targets are forbidden.
import { localTestEndpoint } from "../frontend/e2e/local-test-endpoints.mjs";
const endpoint = localTestEndpoint(
  process.argv[2] ?? "http://127.0.0.1:9229",
  "Cognito seed",
);
async function invoke(operation, body) {
  const response = await fetch(endpoint, {
    method: "POST",
    redirect: "error",
    signal: AbortSignal.timeout(10_000),
    headers: {
      "content-type": "application/x-amz-json-1.1",
      "x-amz-target": `AWSCognitoIdentityProviderService.${operation}`,
    },
    body: JSON.stringify(body),
  });
  if (!response.ok)
    throw new Error(
      `Local Cognito ${operation} failed: HTTP ${response.status}`,
    );
  return response.json();
}
const pool = await invoke("CreateUserPool", {
  PoolName: `dal-audit-${process.pid}`,
});
const poolId = pool.UserPool?.Id;
if (typeof poolId !== "string") throw new Error("Missing local pool ID");
const client = await invoke("CreateUserPoolClient", {
  UserPoolId: poolId,
  ClientName: "dal-audit",
  GenerateSecret: false,
  ExplicitAuthFlows: ["ALLOW_USER_PASSWORD_AUTH", "ALLOW_REFRESH_TOKEN_AUTH"],
});
const clientId = client.UserPoolClient?.ClientId;
if (typeof clientId !== "string") throw new Error("Missing local client ID");
// These are public test identifiers, not credentials. Never print the provider response.
console.log(JSON.stringify({ endpoint, poolId, clientId }));

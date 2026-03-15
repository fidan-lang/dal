import { chromium } from "@playwright/test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const frontendRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(frontendRoot, "..");

const webBaseUrl = process.env.DAL_WEB_BASE_URL ?? "https://dal.fidan.dev";
const apiBaseUrl = process.env.DAL_API_BASE_URL ?? "https://api.dal.fidan.dev";

function logStep(message) {
  console.log(`\n[smoke] ${message}`);
}

async function readCredentials() {
  const raw = await readFile(path.join(repoRoot, "LOCAL", "mock_account.json"), "utf8");
  const parsed = JSON.parse(raw);
  return {
    username: parsed.username,
    email: parsed.email,
    password: parsed.password,
  };
}

async function createSmokeArchive(packageName) {
  const dir = await mkdtemp(path.join(tmpdir(), "dal-live-smoke-"));
  const srcDir = path.join(dir, "src");
  const archivePath = path.join(dir, `${packageName}-0.1.0.tar.gz`);
  await mkdir(srcDir, { recursive: true });

  const baseManifest = await readFile(
    path.join(repoRoot, "LOCAL", "publish-smoke", "dal.toml"),
    "utf8",
  );
  const mainSource = await readFile(
    path.join(repoRoot, "LOCAL", "publish-smoke", "src", "main.fdn"),
    "utf8",
  );

  await writeFile(
    path.join(dir, "dal.toml"),
    baseManifest.replace('name = "playwright-smoke"', `name = "${packageName}"`),
    "utf8",
  );
  await writeFile(path.join(srcDir, "main.fdn"), mainSource, "utf8");

  execFileSync("tar", ["-czf", archivePath, "-C", dir, "dal.toml", "src/main.fdn"]);

  return {
    archivePath,
    cleanup: () => rm(dir, { recursive: true, force: true }),
  };
}

async function login(page, username, password) {
  logStep("Logging in through the real site");
  await page.goto(`${webBaseUrl}/login`, { waitUntil: "networkidle" });
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.waitForLoadState("networkidle");

  const urlAfterLogin = page.url();
  const navText = (await page.locator("nav").textContent()) ?? "";

  return { urlAfterLogin, navText };
}

function parseCookieValue(setCookie, name) {
  const match = setCookie.match(new RegExp(`^${name}=([^;]+)`));
  return match?.[1] ?? null;
}

async function establishSessionViaApi(context, username, password) {
  logStep("Establishing a browser session via the login API to continue smoke testing");
  const response = await fetch(`${apiBaseUrl}/auth/login`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify({ username, password }),
  });

  if (!response.ok) {
    throw new Error(`login api failed with ${response.status}`);
  }

  const cookies = response.headers.getSetCookie?.() ?? [];
  const accessToken = cookies
    .map((cookie) => parseCookieValue(cookie, "dal_access_token"))
    .find(Boolean);
  const refreshToken = cookies
    .map((cookie) => parseCookieValue(cookie, "dal_refresh_token"))
    .find(Boolean);

  if (!accessToken) {
    throw new Error("login api did not return dal_access_token");
  }

  const cookieHeader = [
    `dal_access_token=${accessToken}`,
    refreshToken ? `dal_refresh_token=${refreshToken}` : null,
  ]
    .filter(Boolean)
    .join("; ");

  const browserCookies = [
    {
      name: "dal_access_token",
      value: accessToken,
      domain: "dal.fidan.dev",
      path: "/",
      httpOnly: true,
      secure: true,
      sameSite: "Strict",
    },
    {
      name: "dal_access_token",
      value: accessToken,
      domain: "api.dal.fidan.dev",
      path: "/",
      httpOnly: true,
      secure: true,
      sameSite: "Strict",
    },
  ];

  if (refreshToken) {
    browserCookies.push({
      name: "dal_refresh_token",
      value: refreshToken,
      domain: "dal.fidan.dev",
      path: "/auth/refresh",
      httpOnly: true,
      secure: true,
      sameSite: "Strict",
    });
    browserCookies.push({
      name: "dal_refresh_token",
      value: refreshToken,
      domain: "api.dal.fidan.dev",
      path: "/auth/refresh",
      httpOnly: true,
      secure: true,
      sameSite: "Strict",
    });
  }

  await context.addCookies(browserCookies);
  return { cookieHeader };
}

async function apiGetMe(cookieHeader) {
  const response = await fetch(`${apiBaseUrl}/auth/me`, {
    headers: {
      cookie: cookieHeader,
    },
  });
  const body = await response.text();
  return { ok: response.ok, status: response.status, body };
}

async function apiUpdateProfile(cookieHeader) {
  const originalResponse = await fetch(`${apiBaseUrl}/auth/me`, {
    headers: { cookie: cookieHeader },
  });
  const original = await originalResponse.json();

  const marker = `Smoke ${Date.now().toString(36)}`;
  const next = {
    display_name: marker,
    bio: `Bio ${marker}`,
    website: `https://example.com/${marker.toLowerCase().replace(/\s+/g, "-")}`,
  };

  const updateResponse = await fetch(`${apiBaseUrl}/users/me/profile`, {
    method: "PATCH",
    headers: {
      cookie: cookieHeader,
      "content-type": "application/json",
    },
    body: JSON.stringify(next),
  });
  const updatedBody = await updateResponse.text();

  const restorePayload = {
    display_name: original.display_name ?? "",
    bio: original.bio ?? "",
    website: original.website_url ?? "",
  };
  const restoreResponse = await fetch(`${apiBaseUrl}/users/me/profile`, {
    method: "PATCH",
    headers: {
      cookie: cookieHeader,
      "content-type": "application/json",
    },
    body: JSON.stringify(restorePayload),
  });

  return {
    ok: updateResponse.ok,
    status: updateResponse.status,
    updatedBody,
    restoreOk: restoreResponse.ok,
    restoreStatus: restoreResponse.status,
  };
}

async function apiCreateToken(cookieHeader) {
  const tokenName = `live-smoke-${Date.now().toString(36)}`;
  const response = await fetch(`${apiBaseUrl}/tokens`, {
    method: "POST",
    headers: {
      cookie: cookieHeader,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      name: tokenName,
      scopes: ["publish:new", "publish:update", "yank", "owner", "user:write"],
    }),
  });
  const bodyText = await response.text();
  let parsed = null;
  try {
    parsed = JSON.parse(bodyText);
  } catch {
    parsed = null;
  }

  return {
    ok: response.ok,
    status: response.status,
    bodyText,
    parsed,
    tokenName,
  };
}

async function apiRevokeToken(cookieHeader, tokenId) {
  const response = await fetch(`${apiBaseUrl}/tokens/${tokenId}`, {
    method: "DELETE",
    headers: {
      cookie: cookieHeader,
    },
  });
  const bodyText = await response.text();
  return { ok: response.ok, status: response.status, bodyText };
}

async function apiInviteSelf(cookieHeader, packageName, username) {
  const response = await fetch(`${apiBaseUrl}/packages/${packageName}/owners/invite`, {
    method: "POST",
    headers: {
      cookie: cookieHeader,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      username,
      role: "collaborator",
    }),
  });
  const bodyText = await response.text();
  return { ok: response.ok, status: response.status, bodyText };
}

async function ensureDashboard(page) {
  await page.goto(`${webBaseUrl}/dashboard`, { waitUntil: "networkidle" });
  const heading = page.getByRole("heading", { name: "Dashboard" });
  const isVisible = await heading.isVisible().catch(() => false);
  return {
    ok: isVisible,
    url: page.url(),
    title: await page.title(),
  };
}

async function testProfile(page) {
  logStep("Testing profile save and reload");
  await page.goto(`${webBaseUrl}/settings`, { waitUntil: "networkidle" });

  const displayInput = page.getByLabel("Display name");
  const bioInput = page.getByLabel("Bio");
  const websiteInput = page.getByLabel("Website URL");

  const original = {
    displayName: await displayInput.inputValue(),
    bio: await bioInput.inputValue(),
    website: await websiteInput.inputValue(),
  };

  const marker = `Smoke ${Date.now().toString(36)}`;
  const next = {
    displayName: marker,
    bio: `Bio ${marker}`,
    website: `https://example.com/${marker.toLowerCase().replace(/\s+/g, "-")}`,
  };

  await displayInput.fill(next.displayName);
  await bioInput.fill(next.bio);
  await websiteInput.fill(next.website);
  await page.getByRole("button", { name: "Save profile" }).click();
  await page.waitForLoadState("networkidle");

  const successVisible = await page.getByText("Profile saved.").isVisible().catch(() => false);
  const errorText =
    (await page
      .locator("text=/Failed to save profile|forbidden|unauthorized|validation/i")
      .first()
      .textContent()
      .catch(() => null)) ?? null;

  let persisted = null;
  if (successVisible) {
    await page.reload({ waitUntil: "networkidle" });
    persisted = {
      displayName: await displayInput.inputValue(),
      bio: await bioInput.inputValue(),
      website: await websiteInput.inputValue(),
    };
  }

  const ok =
    successVisible &&
    persisted?.displayName === next.displayName &&
    persisted?.bio === next.bio &&
    persisted?.website === next.website;

  if (successVisible) {
    await displayInput.fill(original.displayName);
    await bioInput.fill(original.bio);
    await websiteInput.fill(original.website);
    await page.getByRole("button", { name: "Save profile" }).click();
    await page.waitForLoadState("networkidle");
  }

  return { ok, original, next, persisted, errorText, url: page.url() };
}

async function createApiToken(page) {
  logStep("Testing API token creation");
  await page.goto(`${webBaseUrl}/settings/tokens`, { waitUntil: "networkidle" });
  const tokenName = `live-smoke-${Date.now().toString(36)}`;

  await page.getByPlaceholder("Token name, e.g. CI deploy").fill(tokenName);
  await page.getByRole("button", { name: "New token" }).click();
  await page.waitForLoadState("networkidle");

  const createdVisible = await page
    .getByText("Token created. Copy it now because it will not be shown again.")
    .isVisible()
    .catch(() => false);
  const token = createdVisible
    ? ((await page.locator("code").first().textContent())?.trim() ?? "")
    : "";
  const tokenListed = await page.getByText(tokenName, { exact: true }).isVisible().catch(() => false);
  const errorText =
    (await page
      .locator("text=/Failed to create token|forbidden|unauthorized|validation/i")
      .first()
      .textContent()
      .catch(() => null)) ?? null;

  return { tokenName, token, tokenListed, createdVisible, errorText, url: page.url() };
}

async function publishPackage(token) {
  logStep("Testing package publish with generated API token");
  const packageName = `live-smoke-${Date.now().toString(36)}`;
  const archive = await createSmokeArchive(packageName);

  try {
    const form = new FormData();
    const buffer = readFileSync(archive.archivePath);
    form.append(
      "archive",
      new Blob([buffer], { type: "application/gzip" }),
      `${packageName}-0.1.0.tar.gz`,
    );

    const response = await fetch(`${apiBaseUrl}/packages/${packageName}/publish`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
      },
      body: form,
    });

    const bodyText = await response.text();
    return {
      ok: response.ok,
      status: response.status,
      bodyText,
      packageName,
    };
  } finally {
    await archive.cleanup();
  }
}

async function verifyPublishedPackage(page, packageName) {
  logStep("Verifying published package pages");
  await page.goto(`${webBaseUrl}/packages/${packageName}`, { waitUntil: "networkidle" });
  const packageHeadingVisible = await page
    .getByRole("heading", { name: packageName })
    .isVisible();
  const packageDescriptionVisible = await page
    .getByText("A live end-to-end smoke package")
    .isVisible()
    .catch(() => false);

  await page.goto(`${webBaseUrl}/dashboard/packages/${packageName}`, {
    waitUntil: "networkidle",
  });
  const manageHeading = await page
    .getByRole("heading", { name: `Manage ${packageName}` })
    .isVisible()
    .catch(() => false);

  return { packageHeadingVisible, packageDescriptionVisible, manageHeading };
}

async function testInviteSelfValidation(page, username, packageName) {
  logStep("Testing invite flow validation with self-invite");
  await page.goto(`${webBaseUrl}/dashboard/packages/${packageName}`, {
    waitUntil: "networkidle",
  });
  await page.getByLabel("Username").fill(username);
  await page.getByRole("button", { name: "Send invite" }).click();
  await page.waitForLoadState("networkidle");

  const errorTexts = [
    "you cannot invite yourself",
    "Failed to send invite.",
  ];

  for (const text of errorTexts) {
    const visible = await page.getByText(text, { exact: false }).isVisible().catch(() => false);
    if (visible) return { ok: true, errorText: text };
  }

  return { ok: false, errorText: null };
}

async function revokeToken(page, tokenName) {
  logStep("Testing API token revocation");
  await page.goto(`${webBaseUrl}/settings/tokens`, { waitUntil: "networkidle" });
  const tokenCard = page.locator("div").filter({ hasText: tokenName }).first();
  const revokeButton = tokenCard.getByRole("button", { name: "Revoke" });
  await revokeButton.click();
  await page.waitForLoadState("networkidle");
  const stillVisible = await page.getByText(tokenName, { exact: true }).isVisible().catch(() => false);
  return { ok: !stillVisible, url: page.url() };
}

async function main() {
  const credentials = await readCredentials();
  const browser = await chromium.launch({
    headless: true,
    channel: process.platform === "win32" ? "chrome" : undefined,
  });
  const context = await browser.newContext();
  const page = await context.newPage();

  try {
    const loginResult = await login(page, credentials.username, credentials.password);
    let dashboardResult = await ensureDashboard(page);
    let sessionDebug = null;

    if (!dashboardResult.ok) {
      sessionDebug = await establishSessionViaApi(context, credentials.username, credentials.password);
      dashboardResult = await ensureDashboard(page);
    }

    const profileResult = await testProfile(page);
    const tokenResult = await createApiToken(page);
    const apiMeResult = sessionDebug ? await apiGetMe(sessionDebug.cookieHeader) : null;
    const apiProfileResult = sessionDebug ? await apiUpdateProfile(sessionDebug.cookieHeader) : null;
    const apiTokenResult = sessionDebug ? await apiCreateToken(sessionDebug.cookieHeader) : null;

    let publishResult = null;
    let packageResult = null;
    let inviteResult = null;
    let revokeResult = null;

    const publishToken =
      tokenResult.token.startsWith("dal_")
        ? tokenResult.token
        : apiTokenResult?.parsed?.token;

    if (publishToken) {
      publishResult = await publishPackage(publishToken);
      packageResult = publishResult.ok
        ? await verifyPublishedPackage(page, publishResult.packageName)
        : null;
      inviteResult = publishResult?.ok && sessionDebug
        ? await apiInviteSelf(sessionDebug.cookieHeader, publishResult.packageName, credentials.username)
        : null;
      if (tokenResult.token.startsWith("dal_")) {
        revokeResult = await revokeToken(page, tokenResult.tokenName);
      } else if (sessionDebug && apiTokenResult?.parsed?.meta?.id) {
        revokeResult = await apiRevokeToken(sessionDebug.cookieHeader, apiTokenResult.parsed.meta.id);
      }
    }

    const summary = {
      login: {
        redirectedToDashboard: /\/dashboard$/.test(loginResult.urlAfterLogin),
        urlAfterLogin: loginResult.urlAfterLogin,
        navShowsUser: loginResult.navText.toLowerCase().includes(credentials.username.toLowerCase()),
      },
      dashboardLoad: dashboardResult,
      apiSession: apiMeResult,
      profile: profileResult,
      apiProfile: apiProfileResult,
      tokens: {
        created: tokenResult.token.startsWith("dal_"),
        listed: tokenResult.tokenListed,
        revoked: revokeResult?.ok ?? null,
        errorText: tokenResult.errorText,
        url: tokenResult.url,
        apiCreated: apiTokenResult?.ok ?? null,
        apiStatus: apiTokenResult?.status ?? null,
      },
      publish: publishResult,
      packagePages: packageResult,
      inviteValidation: inviteResult,
    };

    console.log(`\n[smoke] summary\n${JSON.stringify(summary, null, 2)}`);
  } finally {
    await context.close();
    await browser.close();
  }
}

main().catch((error) => {
  console.error("\n[smoke] failed");
  console.error(error?.stack ?? String(error));
  process.exitCode = 1;
});

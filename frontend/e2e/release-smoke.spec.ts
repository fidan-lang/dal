import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const repoRoot = path.resolve(__dirname, "..", "..");

const apiBaseUrl = process.env.DAL_API_BASE_URL ?? "http://127.0.0.1:8080";
const mailSinkUrl = process.env.DAL_MAIL_SINK_URL ?? "http://127.0.0.1:8025";
const webBaseUrl = process.env.DAL_WEB_BASE_URL ?? "http://localhost:4174";

type MailMessage = {
  body: string;
};

function asMailMessages(payload: unknown): MailMessage[] {
  if (Array.isArray(payload)) {
    return payload.filter(
      (message): message is MailMessage =>
        typeof message === "object" &&
        message !== null &&
        "body" in message &&
        typeof message.body === "string",
    );
  }

  if (
    typeof payload === "object" &&
    payload !== null &&
    "body" in payload &&
    typeof payload.body === "string"
  ) {
    return [payload as MailMessage];
  }

  return [];
}

test.describe.configure({ mode: "serial" });

async function resetMailSink(request: APIRequestContext) {
  await request.post(`${mailSinkUrl}/reset`);
}

async function waitForMailToken(
  request: APIRequestContext,
  routePrefix: string,
  username: string,
) {
  const tokenPattern = new RegExp(`/${routePrefix}\\?token=([A-Za-z0-9_]+)`);

  for (let attempt = 0; attempt < 50; attempt += 1) {
    const response = await request.get(`${mailSinkUrl}/messages`);
    const messages = asMailMessages(await response.json());
    const token = messages
      .filter((message) => message.body.includes(username))
      .map((message) => tokenPattern.exec(message.body)?.[1] ?? null)
      .filter((value): value is string => Boolean(value))
      .at(-1);

    if (token) {
      return token;
    }

    await new Promise((resolve) => setTimeout(resolve, 500));
  }

  throw new Error(`Timed out waiting for ${routePrefix} token for ${username}`);
}

function createSmokeArchive(packageName: string) {
  const dir = mkdtempSync(path.join(tmpdir(), "dal-playwright-"));
  const srcDir = path.join(dir, "src");
  const archivePath = path.join(dir, `${packageName}-0.1.0.tar.gz`);
  const manifestPath = path.join(dir, "dal.toml");
  const sourcePath = path.join(srcDir, "main.fdn");
  mkdirSync(srcDir, { recursive: true });

  const baseManifest = readFileSync(
    path.join(repoRoot, "LOCAL", "publish-smoke", "dal.toml"),
    "utf8",
  );
  const mainSource = readFileSync(
    path.join(repoRoot, "LOCAL", "publish-smoke", "src", "main.fdn"),
    "utf8",
  );

  writeFileSync(
    manifestPath,
    baseManifest.replace(
      'name = "playwright-smoke"',
      `name = "${packageName}"`,
    ),
  );
  writeFileSync(sourcePath, mainSource, { flag: "w" });

  execFileSync("tar", [
    "-czf",
    archivePath,
    "-C",
    dir,
    "dal.toml",
    "src/main.fdn",
  ]);

  return {
    archivePath,
    cleanup: () => rmSync(dir, { recursive: true, force: true }),
  };
}

async function registerViaUi(
  page: Page,
  username: string,
  email: string,
  password: string,
) {
  await page.goto("/register");
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByLabel("Confirm password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(page.getByText("Check your email")).toBeVisible();
}

async function loginViaUi(page: Page, username: string, password: string) {
  await page.goto("/login");
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/dashboard$/);
  await expect(page.getByRole("heading", { name: "Dashboard" })).toBeVisible();
}

test("owner flow, publish flow, and invite acceptance work through the app", async ({
  browser,
  page,
  request,
}) => {
  await resetMailSink(request);

  const suffix = Date.now().toString(36);
  const ownerUsername = `pw-owner-${suffix}`;
  const collaboratorUsername = `pw-collab-${suffix}`;
  const ownerEmail = `${ownerUsername}@example.test`;
  const collaboratorEmail = `${collaboratorUsername}@example.test`;
  const password = "ReleasePass123!";
  const packageName = `pw-smoke-${suffix}`;

  const ownerContext = page.context();
  const collaboratorContext = await browser.newContext({ baseURL: webBaseUrl });
  const collaboratorPage = await collaboratorContext.newPage();

  await registerViaUi(page, ownerUsername, ownerEmail, password);
  const ownerVerifyToken = await waitForMailToken(
    request,
    "verify-email",
    ownerUsername,
  );
  await page.goto(`/verify-email?token=${ownerVerifyToken}`);
  await expect(
    page.getByRole("heading", { name: "Email verified!" }),
  ).toBeVisible();

  await loginViaUi(page, ownerUsername, password);

  await page.goto("/settings/tokens");
  await page
    .getByPlaceholder("Token name, e.g. CI deploy")
    .fill("Playwright publish token");
  await page.getByRole("button", { name: "New token" }).click();
  await expect(
    page.getByText(
      "Token created. Copy it now because it will not be shown again.",
    ),
  ).toBeVisible();
  const publishToken = (
    await page.locator("code").first().textContent()
  )?.trim();
  expect(publishToken).toMatch(/^dal_/);

  const archive = createSmokeArchive(packageName);
  try {
    const publishResponse = await request.post(
      `${apiBaseUrl}/packages/${packageName}/publish`,
      {
        headers: {
          Authorization: `Bearer ${publishToken}`,
        },
        multipart: {
          archive: {
            name: `${packageName}-0.1.0.tar.gz`,
            mimeType: "application/gzip",
            buffer: readFileSync(archive.archivePath),
          },
        },
      },
    );
    expect(publishResponse.ok()).toBeTruthy();
  } finally {
    archive.cleanup();
  }

  await page.goto(`/packages/${packageName}`);
  await expect(page.getByRole("heading", { name: packageName })).toBeVisible();
  await expect(page.getByText("A live end-to-end smoke package")).toBeVisible();

  await registerViaUi(
    collaboratorPage,
    collaboratorUsername,
    collaboratorEmail,
    password,
  );
  const collaboratorVerifyToken = await waitForMailToken(
    request,
    "verify-email",
    collaboratorUsername,
  );
  await collaboratorPage.goto(`/verify-email?token=${collaboratorVerifyToken}`);
  await expect(
    collaboratorPage.getByRole("heading", { name: "Email verified!" }),
  ).toBeVisible();
  await loginViaUi(collaboratorPage, collaboratorUsername, password);

  await page.goto(`/dashboard/packages/${packageName}`);
  await page.getByLabel("Username", { exact: true }).fill(collaboratorUsername);
  await page.getByRole("button", { name: "Send invite" }).click();
  await expect(page.getByText("Invite sent.")).toBeVisible();

  await collaboratorPage.goto("/dashboard");
  await expect(
    collaboratorPage.getByRole("heading", { name: "Pending invites" }),
  ).toBeVisible();
  await expect(collaboratorPage.getByText(packageName)).toBeVisible();
  await collaboratorPage.getByRole("button", { name: "Accept" }).click();
  await expect(collaboratorPage.getByText(packageName)).toBeVisible();

  await collaboratorContext.close();
  await ownerContext.clearCookies();
});

test("forgot-password and reset-password work through the UI", async ({
  page,
  request,
}) => {
  await resetMailSink(request);

  const suffix = `${Date.now().toString(36)}-pw`;
  const username = `pw-reset-${suffix}`;
  const email = `${username}@example.test`;
  const password = "ResetPass123!";
  const newPassword = "ResetPass456!";

  await registerViaUi(page, username, email, password);
  const verifyToken = await waitForMailToken(request, "verify-email", username);
  await page.goto(`/verify-email?token=${verifyToken}`);
  await expect(
    page.getByRole("heading", { name: "Email verified!" }),
  ).toBeVisible();

  await page.goto("/forgot-password");
  await page.getByLabel("Email").fill(email);
  await page.getByRole("button", { name: "Send reset link" }).click();
  await expect(page.getByText("Check your email")).toBeVisible();

  const resetToken = await waitForMailToken(
    request,
    "reset-password",
    username,
  );
  await page.goto(`/reset-password?token=${resetToken}`);
  await page.getByLabel("New password", { exact: true }).fill(newPassword);
  await page
    .getByLabel("Confirm new password", { exact: true })
    .fill(newPassword);
  await page.getByRole("button", { name: "Update password" }).click();
  await expect(
    page.getByRole("heading", { name: "Password updated!" }),
  ).toBeVisible();

  await loginViaUi(page, username, newPassword);
});

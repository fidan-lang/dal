import { localTestEndpoint } from "./local-test-endpoints.mjs";
/// <reference types="node" />

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from "@playwright/test";
import { execFileSync, spawnSync } from "node:child_process";
import {
  cpSync,
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
const frontendFixture = path.join(__dirname, "fixtures", "package");

const apiBaseUrl = localTestEndpoint(
  process.env.DAL_API_BASE_URL ?? "http://127.0.0.1:18080",
  "API",
);
const mailSinkUrl = localTestEndpoint(
  process.env.DAL_MAIL_SINK_URL ?? "http://127.0.0.1:8025",
  "mail sink",
);
const webBaseUrl = localTestEndpoint(
  process.env.DAL_WEB_BASE_URL ?? "http://127.0.0.1:4174",
  "web",
);

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
  const rootDirName = `${packageName}-0.1.0`;
  const packageDir = path.join(dir, rootDirName);
  const srcDir = path.join(packageDir, "src");
  const archivePath = path.join(dir, `${packageName}-0.1.0.tar.gz`);
  const manifestPath = path.join(packageDir, "dal.toml");
  const readmePath = path.join(packageDir, "README.md");
  const sourcePath = path.join(srcDir, "init.fdn");
  mkdirSync(srcDir, { recursive: true });

  const baseManifest = readFileSync(
    path.join(frontendFixture, "dal.toml"),
    "utf8",
  );
  const mainSource = readFileSync(
    path.join(frontendFixture, "src", "init.fdn"),
    "utf8",
  );
  const readme = readFileSync(path.join(frontendFixture, "README.md"), "utf8");

  writeFileSync(
    manifestPath,
    baseManifest.replace(
      'name = "playwright-smoke"',
      `name = "${packageName}"`,
    ),
  );
  writeFileSync(sourcePath, mainSource, { flag: "w" });
  writeFileSync(readmePath, readme, { flag: "w" });

  execFileSync("tar", ["-czf", archivePath, "-C", dir, rootDirName]);

  return {
    archivePath,
    cleanup: () => rmSync(dir, { recursive: true, force: true }),
  };
}

function actualCliLifecycle(token: string, suffix: string) {
  const executable = process.env.DAL_FIDAN_BIN;
  if (!executable)
    throw new Error(
      "DAL_FIDAN_BIN must point to the locally built Fidan executable for release E2E",
    );
  const argsSource = process.env.DAL_ARGS_PACKAGE;
  if (!argsSource)
    throw new Error("DAL_ARGS_PACKAGE must identify the local args candidate");
  // The staged package was capability-probed against this exact compiler.
  // Stable-only semver ranges deliberately do not opt in to an unpublished RC.
  const requirement = /^fidan\s*=\s*"([^"\n]+)"/m.exec(
    readFileSync(path.join(argsSource, "dal.toml"), "utf8"),
  )?.[1];
  expect(requirement).toMatch(/^=\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
  const scratch = mkdtempSync(path.join(tmpdir(), "dal-cli-lifecycle-"));
  const project = path.join(scratch, "publisher");
  const consumer = path.join(scratch, "consumer");
  const name = `cli-smoke-${suffix}`;
  const env = {
    ...process.env,
    FIDAN_HOME: path.join(scratch, "home"),
    FIDAN_DAL_REGISTRY: apiBaseUrl,
    FIDAN_DAL_API_TOKEN: token,
  };
  const invoke = (
    cwd: string,
    args: string[],
    succeeds = true,
    extraEnv: NodeJS.ProcessEnv = {},
  ) => {
    const result = spawnSync(executable, args, {
      cwd,
      env: { ...env, ...extraEnv },
      encoding: "utf8",
      timeout: 120_000,
      windowsHide: true,
    });
    expect(result.error, "CLI process must complete").toBeUndefined();
    expect(
      result.status === 0,
      `CLI ${args.slice(0, 2).join(" ")} exit contract`,
    ).toBe(succeeds);
    return result;
  };
  const consumerManifest = '[package]\nname="consumer"\nversion="0.1.0"\n';
  try {
    mkdirSync(path.join(project, "src"), { recursive: true });
    mkdirSync(consumer);
    writeFileSync(
      path.join(project, "dal.toml"),
      `[package]\nname="${name}"\nversion="0.1.0"\nfidan="${requirement}"\n`,
    );
    writeFileSync(
      path.join(project, "src/init.fdn"),
      "action value returns integer { return 42 }\n",
    );
    writeFileSync(path.join(consumer, "dal.toml"), consumerManifest);
    invoke(project, ["dal", "whoami"]);
    invoke(project, [
      "dal",
      "package",
      ".",
      "--output",
      path.join(scratch, "package.tar.gz"),
    ]);
    invoke(project, ["dal", "publish"]);
    invoke(project, ["dal", "publish"], false);
    invoke(consumer, ["dal", "search", name]);
    invoke(consumer, ["dal", "info", name]);
    invoke(consumer, ["dal", "add", name]);
    const module = name.replaceAll("-", "_");
    writeFileSync(
      path.join(consumer, "main.fdn"),
      `use ${module}\nprint(${module}.value())\n`,
    );
    expect(invoke(consumer, ["run", "main.fdn"]).stdout.trim()).toBe("42");
    invoke(consumer, [
      "build",
      "main.fdn",
      "--backend",
      "cranelift",
      "-o",
      path.join(scratch, "compiled"),
    ]);
    const compiled = path.join(
      scratch,
      process.platform === "win32" ? "compiled.exe" : "compiled",
    );
    expect(
      execFileSync(compiled, [], {
        cwd: consumer,
        env,
        encoding: "utf8",
      }).trim(),
    ).toBe("42");
    const lock = readFileSync(path.join(consumer, "dal.lock"), "utf8");
    expect(lock).toContain("checksum");
    invoke(project, ["dal", "yank", name, "0.1.0"]);
    invoke(consumer, ["dal", "add", name]);
    expect(readFileSync(path.join(consumer, "dal.lock"), "utf8")).toBe(lock);
    const fresh = path.join(scratch, "fresh");
    mkdirSync(fresh);
    writeFileSync(path.join(fresh, "dal.toml"), consumerManifest);
    invoke(fresh, ["dal", "add", name], false);
    invoke(project, ["dal", "unyank", name, "0.1.0"]);
    invoke(fresh, ["dal", "add", name]);
    invoke(consumer, ["dal", "add", name, "--global"]);
    invoke(consumer, ["dal", "remove", name, "--global", "--confirm"]);
    invoke(consumer, [
      "dal",
      "add",
      name,
      "--into",
      path.join(scratch, "vendor"),
    ]);
    invoke(consumer, [
      "dal",
      "remove",
      name,
      "--into",
      path.join(scratch, "vendor"),
      "--confirm",
    ]);
    invoke(consumer, ["dal", "remove", name, "--confirm"]);
    expect(readFileSync(path.join(consumer, "dal.toml"), "utf8")).not.toContain(
      name,
    );
    const argsPublisher = path.join(scratch, "args-publisher");
    mkdirSync(argsPublisher);
    for (const item of [
      "dal.toml",
      "src",
      "README.md",
      "LICENSE",
      "NOTICE",
      "assets",
    ]) {
      cpSync(path.join(argsSource, item), path.join(argsPublisher, item), {
        recursive: true,
      });
    }
    invoke(argsPublisher, [
      "dal",
      "package",
      ".",
      "--output",
      path.join(scratch, "args-0.1.0.tar.gz"),
    ]);
    invoke(argsPublisher, ["dal", "publish"]);
    invoke(consumer, ["dal", "add", "args"]);
    const parserProgram =
      'use args.{command, run, ArgumentError}\n@command\naction greet with (optional name oftype string = "", optional verbose oftype boolean = false) {\nassert_eq(name, "Dal")\nassert_eq(verbose, true)\nprint("args installed parser passed")\n}\nrun(greet)\nvar caught = false\nattempt { run(greet, argv set ["--invalid"]) } catch error oftype ArgumentError {\nassert_eq(error.message.contains("Unknown option"), true)\ncaught = true\n}\nassert_eq(caught, true)\n';
    writeFileSync(path.join(consumer, "main.fdn"), parserProgram);
    for (const separator of [[], ["--"]]) {
      expect(
        invoke(consumer, [
          "run",
          "main.fdn",
          ...separator,
          "--name",
          "Dal",
          "--verbose",
        ]).stdout.trim(),
      ).toBe("args installed parser passed");
    }
    invoke(consumer, [
      "build",
      "main.fdn",
      "--backend",
      "cranelift",
      "-o",
      path.join(scratch, "args-compiled"),
    ]);
    const argsBinary = path.join(
      scratch,
      process.platform === "win32" ? "args-compiled.exe" : "args-compiled",
    );
    expect(
      execFileSync(argsBinary, ["--name", "Dal", "--verbose"], {
        cwd: consumer,
        env,
        encoding: "utf8",
      }).trim(),
    ).toBe("args installed parser passed");
    if (process.env.DAL_AUDIT_LLVM_HOME) {
      const llvmBinary = path.join(
        scratch,
        process.platform === "win32" ? "args-llvm.exe" : "args-llvm",
      );
      invoke(
        consumer,
        [
          "build",
          "--backend",
          "llvm",
          "--lto",
          "full",
          "main.fdn",
          "-o",
          llvmBinary,
        ],
        true,
        { FIDAN_HOME: process.env.DAL_AUDIT_LLVM_HOME },
      );
      expect(
        execFileSync(llvmBinary, ["--name", "Dal", "--verbose"], {
          cwd: consumer,
          env,
          encoding: "utf8",
        }).trim(),
      ).toBe("args installed parser passed");
    }
    // The installed package must compose multi-command groups, not just greet.
    const groupedProgram = `use args.{App, ArgumentError}
action deploy with (certain environment oftype string) returns string {
    return "Deploy " + environment
}
var operations = App()
operations.add("deploy", deploy)
var application = App("consumer", "1.0")
application.group("ops", operations, aliases = ["operations"])
assert_eq(application.run(["operations", "deploy", "--environment", "prod"]), "Deploy prod")
assert_eq(application.complete(["ops", "d"]), ["deploy"])
var caught = false
attempt { application.run(["ops", "missing"]) } catch error oftype ArgumentError {
    caught = error.message.contains("Unknown command")
}
assert_eq(caught, true)
print("args installed groups passed")
`;
    writeFileSync(path.join(consumer, "groups.fdn"), groupedProgram);
    for (const threshold of ["0", "1"]) {
      expect(
        invoke(consumer, [
          "run",
          "--jit-threshold",
          threshold,
          "groups.fdn",
        ]).stdout.trim(),
      ).toBe("args installed groups passed");
    }
    for (const backend of [
      "cranelift",
      ...(process.env.DAL_AUDIT_LLVM_HOME ? ["llvm"] : []),
    ]) {
      const binary = path.join(
        scratch,
        `args-groups-${backend}${process.platform === "win32" ? ".exe" : ""}`,
      );
      invoke(
        consumer,
        [
          "build",
          "--backend",
          backend,
          ...(backend === "llvm" ? ["--lto", "full"] : []),
          "groups.fdn",
          "-o",
          binary,
        ],
        true,
        backend === "llvm"
          ? { FIDAN_HOME: process.env.DAL_AUDIT_LLVM_HOME }
          : {},
      );
      expect(
        execFileSync(binary, [], {
          cwd: consumer,
          env,
          encoding: "utf8",
        }).trim(),
      ).toBe("args installed groups passed");
    }
    // Exercise local CLI entry installation without changing the user's persistent PATH.
    const cliPublisher = path.join(scratch, "entry-publisher");
    mkdirSync(path.join(cliPublisher, "src"), { recursive: true });
    const cliName = `cli-entry-${suffix}`;
    writeFileSync(
      path.join(cliPublisher, "dal.toml"),
      `[package]\nname="${cliName}"\nversion="0.1.0"\n[dependencies]\nargs="^0.1"\n[cli]\nentry="src/main.fdn"\nname="args-smoke"\n`,
    );
    cpSync(
      path.join(consumer, "dal.lock"),
      path.join(cliPublisher, "dal.lock"),
    );
    writeFileSync(
      path.join(cliPublisher, "src/init.fdn"),
      'action version returns string {return "0.1.0"}\n',
    );
    writeFileSync(path.join(cliPublisher, "src/main.fdn"), parserProgram);
    invoke(cliPublisher, ["dal", "publish"]);
    invoke(consumer, ["dal", "add", cliName]);
    const entry = path.join(
      consumer,
      ".dal/bin",
      process.platform === "win32" ? "args-smoke.exe" : "args-smoke",
    );
    expect(
      execFileSync(entry, ["--name", "Dal", "--verbose"], {
        cwd: consumer,
        env,
        encoding: "utf8",
      }).trim(),
    ).toBe("args installed parser passed");
    invoke(consumer, ["dal", "remove", cliName, "--confirm"]);
    invoke(consumer, ["dal", "remove", "args", "--confirm"]);
  } finally {
    // Node retries only documented transient Windows busy/permission cleanup errors.
    rmSync(scratch, { recursive: true, force: true, maxRetries: 3 });
  }
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
  const response = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/auth/register" &&
      response.request().method() === "POST",
  );
  await page.getByRole("button", { name: "Create account" }).click();
  expect((await response).status()).toBe(201);
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
  actualCliLifecycle(publishToken!, suffix);
  await page.goto("/settings");
  await page.getByLabel("Display name").fill("Local audit profile");
  await page.getByRole("button", { name: "Save profile" }).click();
  await expect(page.getByText("Profile saved.")).toBeVisible();
  await page.reload();
  await expect(page.getByLabel("Display name")).toHaveValue(
    "Local audit profile",
  );

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

  const renderedPackage = await request.get(
    `${webBaseUrl}/packages/${packageName}`,
  );
  expect(await renderedPackage.text()).not.toContain(
    "<script>globalThis.__dalInjected",
  );
  await page.goto(`/packages/${packageName}`);
  expect(
    await page.evaluate(
      () =>
        (globalThis as typeof globalThis & { __dalInjected?: boolean })
          .__dalInjected,
    ),
  ).toBeUndefined();
  await expect(page.locator('a[href^="javascript:"]')).toHaveCount(0);
  await expect(
    page.locator('a[href="https://example.test/source"]'),
  ).toBeVisible();
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

for (const width of [390, 768, 1440, 1920]) {
  test(`public registry usability at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    for (const route of ["/", "/packages", "/login", "/register"]) {
      const response = await page.goto(route);
      expect(response?.status()).toBe(200);
      await expect(page.locator("main")).toBeVisible();
      await expect(page.locator("h1")).toHaveCount(1);
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth + 1,
        ),
      ).toBe(true);
      const unlabeled = await page
        .locator("input:not([type=hidden])")
        .evaluateAll(
          (inputs) =>
            inputs.filter((input) => {
              const element = input as HTMLInputElement;
              return (
                !element.labels?.length &&
                !element.getAttribute("aria-label") &&
                !element.getAttribute("aria-labelledby")
              );
            }).length,
        );
      expect(unlabeled).toBe(0);
      if (route === "/")
        await page.screenshot({
          path: test.info().outputPath(`registry-${width}.png`),
        });
    }
    expect(errors).toEqual([]);
  });
}

test("footer links software, branding and draft service policies separately", async ({
  page,
  request,
}) => {
  await page.goto("/");
  const legal = page.getByRole("navigation", { name: "Legal", exact: true });
  for (const [name, url] of [
    ["Apache 2.0", "/LICENSE.txt"],
    ["Attribution", "/NOTICE.txt"],
    ["Trademarks", "https://github.com/fidan-lang/dal/blob/main/TRADEMARKS.md"],
    [
      "Registry terms (draft)",
      "https://github.com/fidan-lang/dal/blob/main/REGISTRY_TERMS.md",
    ],
  ]) {
    const link = legal.getByRole("link", { name, exact: true });
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute("href", url);
  }
  for (const name of ["LICENSE", "NOTICE"]) {
    const response = await request.get(`/${name}.txt`);
    expect(response.status()).toBe(200);
    expect(await response.body()).toEqual(
      readFileSync(path.resolve(__dirname, "../..", name)),
    );
  }
});

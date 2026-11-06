<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { goto } from "$app/navigation";
  import QRCode from "qrcode";
  import { mfa } from "$lib/api";
  import { currentUser } from "$lib/stores/auth";
  let enabled = $state<boolean | null>(null);
  let available = $state(false);
  let password = $state("");
  let currentCode = $state("");
  let code = $state("");
  let secret = $state("");
  let uri = $state("");
  let qr = $state("");
  let error = $state("");
  let busy = $state(true);
  let recoveryPassword = $state("");
  let recoveryCurrentCode = $state("");
  let recoveryCodes = $state<string[]>([]);
  let recoveryMessage = $state("");
  // Shown only in memory after successful enrollment. The server has already
  // revoked all sessions; never put these secrets in URLs or local storage.
  let enrollmentCodes = $state<string[]>([]);
  let enrollmentCodesSaved = $state(false);
  let generation = 0;
  let expiry: ReturnType<typeof setTimeout> | undefined;
  function clearSetup() {
    generation++;
    secret = "";
    uri = "";
    qr = "";
    code = "";
    if (expiry) clearTimeout(expiry);
  }
  onDestroy(() => {
    clearSetup();
    recoveryCodes = [];
    enrollmentCodes = [];
  });
  function downloadEnrollmentCodes() {
    if (enrollmentCodes.length !== 10) return;
    const body = [
      "DAL MFA recovery codes",
      "Each code can be used once to reset your authenticator after password verification.",
      "Store this file offline in a protected location. Never share these codes.",
      "",
      ...enrollmentCodes,
      "",
    ].join("\n");
    const url = URL.createObjectURL(
      new Blob([body], { type: "text/plain;charset=utf-8" }),
    );
    const link = document.createElement("a");
    link.href = url;
    link.download = "dal-mfa-recovery-codes.txt";
    document.body.appendChild(link);
    link.click();
    link.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  async function continueAfterEnrollment() {
    if (!enrollmentCodesSaved || enrollmentCodes.length !== 10) return;
    enrollmentCodes = [];
    enrollmentCodesSaved = false;
    await goto("/login", { invalidateAll: true });
  }
  async function generateRecoveryCodes() {
    busy = true;
    error = "";
    recoveryCodes = [];
    recoveryMessage = "";
    try {
      const result = await mfa.generateRecoveryCodes(
        fetch,
        recoveryPassword,
        recoveryCurrentCode,
      );
      recoveryCodes = result.codes;
      recoveryMessage = result.message;
    } catch {
      error =
        "Unable to generate recovery codes. Use a fresh password and authenticator code.";
    } finally {
      recoveryPassword = "";
      recoveryCurrentCode = "";
      busy = false;
    }
  }
  onMount(async () => {
    try {
      const status = await mfa.status(fetch);
      enabled = status.enabled;
      available = status.available;
    } catch {
      error =
        "Unable to load authenticator status. Try again before making changes.";
    } finally {
      busy = false;
    }
  });
  async function setup() {
    busy = true;
    error = "";
    clearSetup();
    const epoch = generation;
    try {
      const result = await mfa.setup(
        fetch,
        password,
        enabled ? currentCode : undefined,
      );
      if (epoch !== generation) return;
      password = "";
      currentCode = "";
      secret = result.secret;
      uri = result.uri;
      const generated = await QRCode.toDataURL(uri, {
        width: 240,
        margin: 2,
      });
      if (epoch !== generation) return;
      qr = generated;
      expiry = setTimeout(() => {
        clearSetup();
        error = "Setup expired. Authenticate again to start a new setup.";
      }, result.expires_in * 1000);
    } catch {
      error =
        "Setup failed. Check your password and, if enabled, a fresh authenticator code. Restart setup after five failed verification attempts.";
    } finally {
      busy = false;
      password = "";
      currentCode = "";
    }
  }
  async function finish(disable = false) {
    busy = true;
    error = "";
    try {
      if (disable) {
        await mfa.disable(fetch, password, currentCode);
        clearSetup();
        currentUser.set(null);
        await goto("/login", { invalidateAll: true });
      } else {
        const result = await mfa.verify(fetch, code);
        clearSetup();
        currentUser.set(null);
        // Cognito sessions have already been revoked. Keep only the one-time
        // response in component memory until the user confirms secure storage.
        if (!Array.isArray(result.codes) || result.codes.length !== 10) {
          error =
            "Authenticator enabled, but backup codes were not returned. Sign in again and generate a set in Security settings.";
          return;
        }
        enrollmentCodes = result.codes;
        enrollmentCodesSaved = false;
      }
      password = "";
      currentCode = "";
    } catch {
      error =
        "Change failed. Check your credentials and use a fresh six-digit code. If setup expired or five attempts failed, start again.";
    } finally {
      busy = false;
      password = "";
      currentCode = "";
      code = "";
    }
  }
</script>

<svelte:head
  ><title>Account security — Dal</title><meta
    name="robots"
    content="noindex,nofollow"
  /></svelte:head
>
<div class="max-w-lg space-y-5">
  {#if enrollmentCodes.length === 10}
    <section class="space-y-4" aria-labelledby="enrollment-recovery-heading">
      <h2
        id="enrollment-recovery-heading"
        class="text-lg font-semibold text-white"
      >
        Save your recovery codes
      </h2>
      <p class="text-sm">
        Your authenticator is enabled and all existing sessions are signed out.
        These ten one-use backup codes are shown only once. Save them now before
        continuing to the login page.
      </p>
      <p class="text-sm text-[var(--color-warning)]">
        The downloaded file contains sensitive credentials in plain text. Store
        it securely offline and remove unprotected copies from Downloads.
      </p>
      <pre
        class="overflow-x-auto select-all rounded-[var(--radius-md)] bg-[var(--color-surface-3)] p-3 text-xs">{enrollmentCodes.join(
          "\n",
        )}</pre>
      <button
        type="button"
        onclick={downloadEnrollmentCodes}
        class="rounded-[var(--radius-md)] bg-[var(--color-primary)] px-4 py-2 font-medium text-white"
        >Download recovery codes</button
      >
      <label class="flex items-start gap-2 text-sm">
        <input
          type="checkbox"
          bind:checked={enrollmentCodesSaved}
          class="mt-1"
        />
        <span>I have saved these codes in a secure place.</span>
      </label>
      <button
        type="button"
        disabled={!enrollmentCodesSaved}
        onclick={() => void continueAfterEnrollment()}
        class="rounded-[var(--radius-md)] border border-[var(--color-border)] px-4 py-2 disabled:opacity-50"
        >Continue to sign in</button
      >
      <p class="text-xs text-[var(--color-text-muted)]">
        Closing or refreshing this page will discard the displayed codes. If
        that happens, sign in using your authenticator and generate a new set in
        Security settings.
      </p>
    </section>
  {:else}
    <h2 class="text-lg font-semibold text-white">Authenticator security</h2>
    <p>
      Authenticator MFA is {enabled === null
        ? "unknown until status loads"
        : enabled
          ? "enabled"
          : "not enabled"}. MFA is optional. Downloads never require an account.
    </p>
    {#if !available}<p>
        New authenticator enrollment is not yet available on this registry.
      </p>{/if}
    {#if error}<p role="alert" class="text-[var(--color-danger)]">
        {error}
      </p>{/if}
    {#if secret}
      <p>
        Scan this QR code in your authenticator, or enter the secret manually.
        Keep an offline backup in a safe place. This secret is shown only during
        setup.
      </p>
      {#if qr}<img
          src={qr}
          width="240"
          height="240"
          alt="Scan to add your DAL authenticator"
        />{/if}
      <p>
        Manual setup secret: <code class="break-all select-all">{secret}</code>
      </p>
      <p>Account: DAL. Six digits, 30-second interval, SHA-1.</p>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          void finish();
        }}
        class="space-y-4"
      >
        <label for="new-code">Code from the new authenticator</label>
        <input
          id="new-code"
          type="text"
          bind:value={code}
          pattern={"[0-9]{6}"}
          inputmode="numeric"
          autocomplete="one-time-code"
          maxlength="6"
          required
          class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
        />
        <button
          disabled={busy}
          class="px-5 py-2 bg-[var(--color-primary)] rounded-[var(--radius-md)]"
          >Verify and enable</button
        >
      </form>
      <button disabled={busy} onclick={clearSetup}>Hide setup secret</button>
    {:else}
      <form
        onsubmit={(event) => {
          event.preventDefault();
          void setup();
        }}
        class="space-y-4"
      >
        <label for="security-password">Current password</label>
        <input
          id="security-password"
          type="password"
          bind:value={password}
          autocomplete="current-password"
          required
          class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
        />
        {#if enabled}
          <label for="current-code">Current authenticator code</label>
          <input
            id="current-code"
            type="text"
            bind:value={currentCode}
            pattern={"[0-9]{6}"}
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="6"
            required
            class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
          />
        {/if}
        <button
          disabled={busy || !available}
          class="px-5 py-2 bg-[var(--color-primary)] rounded-[var(--radius-md)]"
          >{enabled ? "Replace authenticator" : "Set up authenticator"}</button
        >
        {#if enabled}<button
            type="button"
            disabled={busy || !password || !/^[0-9]{6}$/.test(currentCode)}
            onclick={() => {
              void finish(true);
            }}
            class="px-5 py-2 text-[var(--color-danger)]"
            >Disable authenticator</button
          >{/if}
      </form>
    {/if}
    {#if enabled}
      <section
        class="space-y-3 border border-[var(--color-border)] rounded-[var(--radius-md)] p-4"
      >
        <h3 class="font-semibold">Backup recovery codes</h3>
        <p class="text-sm">
          Generate ten single-use backup codes. Creating a new set invalidates
          the old one. Keep them offline: each is displayed only once. You need
          your password and current authenticator code.
        </p>
        <form
          class="space-y-3"
          onsubmit={(event) => {
            event.preventDefault();
            void generateRecoveryCodes();
          }}
        >
          <label for="recovery-password">Current password</label>
          <input
            id="recovery-password"
            class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
            type="password"
            autocomplete="current-password"
            bind:value={recoveryPassword}
            required
          />
          <label for="recovery-totp">Current authenticator code</label>
          <input
            id="recovery-totp"
            class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
            type="text"
            inputmode="numeric"
            pattern={"[0-9]{6}"}
            maxlength="6"
            autocomplete="one-time-code"
            bind:value={recoveryCurrentCode}
            required
          />
          <button
            type="submit"
            disabled={busy}
            class="px-5 py-2 bg-[var(--color-primary)] rounded-[var(--radius-md)]"
          >
            Generate recovery codes
          </button>
        </form>
        {#if recoveryMessage}<p role="status" class="text-sm">
            {recoveryMessage}
          </p>{/if}
        {#if recoveryCodes.length > 0}
          <pre
            class="overflow-x-auto select-all rounded-[var(--radius-md)] p-3 bg-[var(--color-surface-3)] text-xs">{recoveryCodes.join(
              "\n",
            )}</pre>
          <button
            type="button"
            onclick={() =>
              navigator.clipboard.writeText(recoveryCodes.join("\n"))}
            class="px-4 py-2 border border-[var(--color-border)] rounded-[var(--radius-md)]"
            >Copy codes</button
          >
          <button
            type="button"
            onclick={() => {
              recoveryCodes = [];
              recoveryMessage = "";
            }}
            class="px-4 py-2 border border-[var(--color-border)] rounded-[var(--radius-md)]"
            >Hide codes</button
          >
        {/if}
      </section>
    {/if}
    <p>
      Changes require fresh password authentication and, when MFA is enabled,
      your current authenticator code. Successful changes sign out all browser
      sessions. Use the next code to sign in again.
    </p>
    <p>
      Lost authenticator? Restore your securely stored authenticator backup.
      Otherwise contact the registry operator for independent identity
      verification. Email access or a password reset alone cannot disable MFA;
      recovery can be refused when identity cannot be established.
    </p>
  {/if}
</div>

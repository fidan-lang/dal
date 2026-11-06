<script lang="ts">
  import { goto } from "$app/navigation";
  import { auth, mfa } from "$lib/api";
  import { currentUser } from "$lib/stores/auth";

  let challenge = $state(false);
  let recoveryMode = $state(false);
  let recoveryNotice = $state("");
  let code = $state("");
  let expiresAt = $state(0);
  let username = $state("");
  let password = $state("");
  let error = $state("");
  let loading = $state(false);
  let showResend = $state(false);
  let resendLoading = $state(false);
  let resendMessage = $state("");
  let resendCooldown = $state(0);
  let cooldownTimer: ReturnType<typeof setInterval> | null = null;

  function startResendCooldown(seconds: number) {
    resendCooldown = seconds;
    if (cooldownTimer) clearInterval(cooldownTimer);
    cooldownTimer = setInterval(() => {
      resendCooldown = Math.max(0, resendCooldown - 1);
      if (resendCooldown === 0 && cooldownTimer) {
        clearInterval(cooldownTimer);
        cooldownTimer = null;
      }
    }, 1000);
  }

  async function handleResendVerification() {
    if (!username || resendLoading || resendCooldown > 0) return;

    resendLoading = true;
    resendMessage = "";
    try {
      const identifier = username.trim();
      const res = identifier.includes("@")
        ? await auth.resendVerification(fetch, identifier)
        : await auth.resendVerificationByUsername(fetch, identifier);
      resendMessage =
        res.message ||
        "If your account exists, a verification email has been sent.";
      startResendCooldown(30);
    } catch {
      resendMessage =
        "If your account exists, a verification email has been sent.";
      startResendCooldown(30);
    } finally {
      resendLoading = false;
    }
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    error = "";
    showResend = false;
    resendMessage = "";
    loading = true;
    try {
      if (challenge && Date.now() >= expiresAt) {
        challenge = false;
        throw new Error("Authentication expired. Sign in again.");
      }
      if (challenge && recoveryMode) {
        const result = await mfa.recover(fetch, code.trim());
        challenge = false;
        recoveryMode = false;
        code = "";
        password = "";
        recoveryNotice = result.message;
        return;
      }
      const user = challenge
        ? await mfa.challenge(fetch, code)
        : await auth.login(fetch, username, password);
      code = "";
      if ("mfa_required" in user) {
        password = "";
        challenge = true;
        expiresAt = Date.now() + user.expires_in * 1000;
        return;
      }
      currentUser.set(user);
      // Re-fetch SSR session data after the login cookies have changed.
      await goto("/dashboard", { invalidateAll: true });
    } catch (err) {
      error =
        err instanceof Error ? err.message : "Login failed. Please try again.";
      showResend = error.toLowerCase().includes("verify your email");
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>Sign in — Dal</title>
</svelte:head>

<div
  class="min-h-[calc(100vh-7rem)] flex items-center justify-center px-4 py-12"
>
  <div class="w-full max-w-sm">
    <div class="text-center mb-8">
      <h1 class="text-2xl font-bold text-white">Welcome back</h1>
      <p class="text-[var(--color-text-muted)] text-sm mt-2">
        Sign in to your Dal account
      </p>
    </div>

    {#if recoveryNotice}<p
        role="status"
        class="text-sm text-[var(--color-primary-light)]"
      >
        {recoveryNotice}
      </p>{/if}
    <form onsubmit={handleSubmit} class="space-y-4">
      {#if error}
        <div
          class="px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
        >
          {error}
        </div>
        {#if showResend}
          <div class="mt-3">
            <button
              type="button"
              onclick={handleResendVerification}
              disabled={resendLoading || resendCooldown > 0}
              class="w-full py-2.5 bg-[var(--color-primary)]/20 hover:bg-[var(--color-primary)]/30 border border-[var(--color-primary)]/40 disabled:opacity-60 text-[var(--color-primary-light)] font-medium rounded-[var(--radius-md)] transition-colors text-sm"
            >
              {#if resendLoading}
                Resending verification email...
              {:else if resendCooldown > 0}
                Resend in {resendCooldown}s
              {:else}
                Resend verification email
              {/if}
            </button>
            {#if resendMessage}
              <p class="mt-2 text-xs text-[var(--color-text-muted)]">
                {resendMessage}
              </p>
            {/if}
          </div>
        {/if}
      {/if}

      {#if !challenge}
        <div>
          <label
            for="username"
            class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
            >Username or email</label
          >
          <input
            id="username"
            type="text"
            bind:value={username}
            required
            autocomplete="username"
            class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
            placeholder="username or email"
          />
        </div>

        <div>
          <label
            for="password"
            class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
            >Password</label
          >
          <input
            id="password"
            type="password"
            bind:value={password}
            required
            autocomplete="current-password"
            class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
            placeholder="••••••••"
          />
        </div>
      {:else}
        <label for="totp"
          >{recoveryMode
            ? "One-time recovery code"
            : "Authenticator code"}</label
        >
        <input
          id="totp"
          type="text"
          bind:value={code}
          inputmode={recoveryMode ? "text" : "numeric"}
          autocomplete={recoveryMode ? "off" : "one-time-code"}
          pattern={recoveryMode ? "dal_[0-9a-fA-F]{64}" : "[0-9]{6}"}
          maxlength={recoveryMode ? 68 : 6}
          required
          class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
        />
        <p class="text-sm">
          Enter the six-digit code from your authenticator. This sign-in expires
          after three minutes and allows five attempts.
        </p>
        <button
          type="button"
          onclick={() => {
            challenge = false;
            code = "";
            error = "";
          }}>Start sign-in again</button
        >
        <button
          type="button"
          class="text-sm text-[var(--color-primary)] underline"
          onclick={() => {
            recoveryMode = !recoveryMode;
            code = "";
            error = "";
          }}
        >
          {recoveryMode
            ? "Use authenticator instead"
            : "Use one-time recovery code"}
        </button>
        <p class="text-sm">
          A recovery code resets your authenticator, revokes existing sessions
          and API tokens, and requires a new password sign-in. Otherwise contact
          the registry operator.
        </p>
      {/if}
      <div class="flex justify-end">
        <a
          href="/forgot-password"
          class="text-xs text-[var(--color-primary)] hover:underline"
          >Forgot password?</a
        >
      </div>

      <button
        type="submit"
        disabled={loading}
        class="w-full py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white font-medium rounded-[var(--radius-md)] transition-colors text-sm"
      >
        {loading ? "Signing in…" : "Sign in"}
      </button>
    </form>

    <p class="text-center text-sm text-[var(--color-text-muted)] mt-6">
      Don't have an account?
      <a href="/register" class="text-[var(--color-primary)] hover:underline"
        >Sign up</a
      >
    </p>
  </div>
</div>

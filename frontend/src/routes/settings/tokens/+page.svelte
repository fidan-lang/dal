<script lang="ts">
  import {
    DalApiError,
    TOKEN_SCOPE_OPTIONS,
    tokens as tokensApi,
    type ApiToken,
    type TokenScope,
  } from "$lib/api";
  import { timeAgo } from "$lib/utils";

  const expiryOptions = [
    { label: "Never expires", value: "never", seconds: null },
    { label: "30 days", value: "30d", seconds: 60 * 60 * 24 * 30 },
    { label: "90 days", value: "90d", seconds: 60 * 60 * 24 * 90 },
    { label: "1 year", value: "365d", seconds: 60 * 60 * 24 * 365 },
  ] as const;

  let { data } = $props();
  let tokenList = $state<ApiToken[]>([]);
  let newName = $state("");
  let newToken = $state<string | null>(null);
  let selectedScopes = $state<TokenScope[]>([
    "publish:new",
    "publish:update",
    "yank",
  ]);
  let selectedExpiry = $state<(typeof expiryOptions)[number]["value"]>("never");
  let error = $state("");
  let creating = $state(false);
  let copied = $state(false);

  $effect(() => {
    tokenList = [...data.tokens];
  });

  function toggleScope(scope: TokenScope, checked: boolean) {
    if (checked) {
      selectedScopes = [...new Set([...selectedScopes, scope])];
      return;
    }

    selectedScopes = selectedScopes.filter((value) => value !== scope);
  }

  function formatScope(scope: TokenScope) {
    return (
      TOKEN_SCOPE_OPTIONS.find((option) => option.value === scope)?.label ??
      scope
    );
  }

  async function createToken(e: Event) {
    e.preventDefault();
    error = "";
    newToken = null;
    creating = true;

    try {
      const expiry = expiryOptions.find(
        (option) => option.value === selectedExpiry,
      );
      const result = await tokensApi.create(fetch, newName, {
        scopes: selectedScopes,
        expires_in: expiry?.seconds ?? null,
      });
      newToken = result.token;
      tokenList = [result.meta, ...tokenList];
      newName = "";
      selectedScopes = ["publish:new", "publish:update", "yank"];
      selectedExpiry = "never";
    } catch (err) {
      error =
        err instanceof DalApiError ? err.message : "Failed to create token.";
    } finally {
      creating = false;
    }
  }

  async function deleteToken(id: string) {
    try {
      await tokensApi.delete(fetch, id);
      tokenList = tokenList.filter((t) => t.id !== id);
    } catch (err) {
      error =
        err instanceof DalApiError ? err.message : "Failed to delete token.";
    }
  }

  async function copyToken() {
    if (!newToken) return;
    await navigator.clipboard.writeText(newToken);
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }
</script>

<svelte:head>
  <title>API Tokens - Dal</title>
</svelte:head>

<div>
  <h2 class="text-lg font-semibold text-white mb-1">API Tokens</h2>
  <p class="text-sm text-[var(--color-text-muted)] mb-6">
    Create least-privilege tokens for the CLI, CI, and automation. Tokens are
    only shown once.
  </p>

  {#if error}
    <div
      class="mb-4 px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
    >
      {error}
    </div>
  {/if}

  {#if newToken}
    <div
      class="mb-6 p-4 bg-[var(--color-success)]/8 border border-[var(--color-success)]/30 rounded-[var(--radius-lg)]"
    >
      <p class="text-sm text-[var(--color-success)] font-medium mb-2">
        Token created. Copy it now because it will not be shown again.
      </p>
      <div class="flex items-center gap-2">
        <code
          class="flex-1 font-mono text-xs bg-[var(--color-surface-3)] border border-[var(--color-border)] px-3 py-2 rounded break-all text-[var(--color-text)]"
        >
          {newToken}
        </code>
        <button
          onclick={copyToken}
          class="shrink-0 px-3 py-2 bg-[var(--color-surface-3)] border border-[var(--color-border)] hover:border-[var(--color-primary)] rounded-[var(--radius-sm)] text-xs text-[var(--color-text)] transition-colors"
        >
          {copied ? "Copied!" : "Copy"}
        </button>
      </div>
    </div>
  {/if}

  <form onsubmit={createToken} class="space-y-5 mb-8 max-w-3xl">
    <div class="flex gap-2 flex-col sm:flex-row">
      <input
        type="text"
        bind:value={newName}
        required
        maxlength="64"
        placeholder="Token name, e.g. CI deploy"
        class="flex-1 px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
      />
      <select
        bind:value={selectedExpiry}
        class="sm:w-44 px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
      >
        {#each expiryOptions as option}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
      <button
        type="submit"
        disabled={creating}
        class="px-4 py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white text-sm font-medium rounded-[var(--radius-md)] transition-colors"
      >
        {creating ? "Creating..." : "New token"}
      </button>
    </div>

    <div
      class="p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
    >
      <p class="text-sm font-medium text-white mb-3">Scopes</p>
      <div class="grid gap-3 sm:grid-cols-2">
        {#each TOKEN_SCOPE_OPTIONS as scope}
          <label
            class="flex gap-3 p-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface-3)] cursor-pointer"
          >
            <input
              type="checkbox"
              checked={selectedScopes.includes(scope.value)}
              onchange={(event) =>
                toggleScope(
                  scope.value,
                  (event.currentTarget as HTMLInputElement).checked,
                )}
              class="mt-1"
            />
            <span>
              <span class="block text-sm text-[var(--color-text)] font-medium">
                {scope.label}
              </span>
              <span class="block text-xs text-[var(--color-text-muted)] mt-1">
                {scope.description}
              </span>
            </span>
          </label>
        {/each}
      </div>
    </div>
  </form>

  {#if tokenList.length}
    <div class="space-y-2">
      {#each tokenList as token}
        <div
          class="p-3.5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
        >
          <div class="flex items-start justify-between gap-4">
            <div>
              <p class="text-sm text-[var(--color-text)] font-medium">
                {token.name}
              </p>
              <p
                class="text-xs text-[var(--color-text-muted)] font-mono mt-0.5"
              >
                {token.prefix}...
              </p>
            </div>
            <button
              onclick={() => deleteToken(token.id)}
              class="text-sm text-[var(--color-danger)] hover:underline"
            >
              Revoke
            </button>
          </div>
          <div class="mt-3 flex flex-wrap gap-2">
            {#each token.scopes as scope}
              <span
                class="px-2.5 py-1 rounded-full bg-[var(--color-primary)]/10 text-[var(--color-primary-light)] text-xs border border-[var(--color-primary)]/20"
              >
                {formatScope(scope)}
              </span>
            {/each}
          </div>
          <div
            class="mt-3 flex flex-wrap gap-5 text-xs text-[var(--color-text-muted)]"
          >
            {#if token.last_used_at}
              <span>Used {timeAgo(token.last_used_at)}</span>
            {:else}
              <span>Never used</span>
            {/if}
            <span>Created {timeAgo(token.created_at)}</span>
            {#if token.expires_at}
              <span>Expires {timeAgo(token.expires_at)}</span>
            {:else}
              <span>No expiry</span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <p class="text-sm text-[var(--color-text-muted)]">
      No tokens yet. Create one for publishing, CI, or account automation.
    </p>
  {/if}
</div>

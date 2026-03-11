<script lang="ts">
    import type { ApiToken } from "$lib/api";
    import { DalApiError, tokens as tokensApi } from "$lib/api";
    import { timeAgo } from "$lib/utils";
    import { untrack } from "svelte";

    let { data } = $props();
    let tokenList = $state<ApiToken[]>(untrack(() => [...data.tokens]));
    $effect(() => {
        tokenList = [...data.tokens];
    });
    let newName = $state("");
    let newToken = $state<string | null>(null);
    let error = $state("");
    let creating = $state(false);
    let copied = $state(false);

    async function createToken(e: Event) {
        e.preventDefault();
        error = "";
        newToken = null;
        creating = true;
        try {
            const result = await tokensApi.create(fetch, newName);
            newToken = result.token;
            tokenList = [...tokenList, result.meta];
            newName = "";
        } catch (err) {
            error =
                err instanceof DalApiError
                    ? err.message
                    : "Failed to create token.";
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
                err instanceof DalApiError
                    ? err.message
                    : "Failed to delete token.";
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
    <title>API Tokens — Dal</title>
</svelte:head>

<div>
    <h2 class="text-lg font-semibold text-white mb-1">API Tokens</h2>
    <p class="text-sm text-[var(--color-text-muted)] mb-6">
        Tokens are used with <code class="font-mono">dal</code> CLI to publish packages.
        They are shown only once.
    </p>

    {#if error}
        <div
            class="mb-4 px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
        >
            {error}
        </div>
    {/if}

    <!-- New token reveal banner -->
    {#if newToken}
        <div
            class="mb-6 p-4 bg-[var(--color-success)]/8 border border-[var(--color-success)]/30 rounded-[var(--radius-lg)]"
        >
            <p class="text-sm text-[var(--color-success)] font-medium mb-2">
                Token created — copy it now, it won't be shown again.
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

    <!-- Create form -->
    <form onsubmit={createToken} class="flex gap-2 mb-8 max-w-md">
        <input
            type="text"
            bind:value={newName}
            required
            maxlength="64"
            placeholder="Token name, e.g. CI/CD"
            class="flex-1 px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
        />
        <button
            type="submit"
            disabled={creating}
            class="px-4 py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white text-sm font-medium rounded-[var(--radius-md)] transition-colors"
        >
            {creating ? "Creating…" : "New token"}
        </button>
    </form>

    <!-- Token list -->
    {#if tokenList.length}
        <div class="space-y-2">
            {#each tokenList as token}
                <div
                    class="flex items-center justify-between p-3.5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
                >
                    <div>
                        <p class="text-sm text-[var(--color-text)] font-medium">
                            {token.name}
                        </p>
                        <p
                            class="text-xs text-[var(--color-text-muted)] font-mono mt-0.5"
                        >
                            {token.prefix}…
                        </p>
                    </div>
                    <div
                        class="flex items-center gap-5 text-xs text-[var(--color-text-muted)]"
                    >
                        {#if token.last_used_at}
                            <span>Used {timeAgo(token.last_used_at)}</span>
                        {:else}
                            <span>Never used</span>
                        {/if}
                        <span>Created {timeAgo(token.created_at)}</span>
                        <button
                            onclick={() => deleteToken(token.id)}
                            class="text-[var(--color-danger)] hover:underline"
                        >
                            Revoke
                        </button>
                    </div>
                </div>
            {/each}
        </div>
    {:else}
        <p class="text-sm text-[var(--color-text-muted)]">
            No tokens yet. Create one to use with the CLI.
        </p>
    {/if}
</div>

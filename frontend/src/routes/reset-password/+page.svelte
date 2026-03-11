<script lang="ts">
    import { page } from "$app/state";
    import { auth, DalApiError } from "$lib/api";

    let token = $derived(page.url.searchParams.get("token") ?? "");
    let password = $state("");
    let password2 = $state("");
    let error = $state("");
    let success = $state(false);
    let loading = $state(false);

    async function handleSubmit(e: Event) {
        e.preventDefault();
        error = "";
        if (password !== password2) {
            error = "Passwords do not match.";
            return;
        }
        if (password.length < 8) {
            error = "Password must be at least 8 characters.";
            return;
        }
        loading = true;
        try {
            await auth.resetPassword(fetch, token, password);
            success = true;
        } catch (err) {
            error =
                err instanceof DalApiError
                    ? err.message
                    : "Reset failed. Please try again.";
        } finally {
            loading = false;
        }
    }
</script>

<svelte:head>
    <title>Reset password — Dal</title>
</svelte:head>

<div
    class="min-h-[calc(100vh-7rem)] flex items-center justify-center px-4 py-12"
>
    <div class="w-full max-w-sm">
        {#if !token}
            <div class="text-center">
                <h1 class="text-xl font-bold text-white mb-2">
                    Invalid reset link
                </h1>
                <a
                    href="/forgot-password"
                    class="text-sm text-[var(--color-primary)] hover:underline"
                    >Request a new one</a
                >
            </div>
        {:else if success}
            <div class="text-center">
                <div
                    class="w-16 h-16 bg-[var(--color-success)]/10 rounded-full flex items-center justify-center mx-auto mb-4"
                >
                    <svg
                        class="w-8 h-8 text-[var(--color-success)]"
                        fill="none"
                        stroke="currentColor"
                        viewBox="0 0 24 24"
                    >
                        <path
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            stroke-width="2"
                            d="M5 13l4 4L19 7"
                        />
                    </svg>
                </div>
                <h1 class="text-2xl font-bold text-white mb-2">
                    Password updated!
                </h1>
                <a
                    href="/login"
                    class="mt-4 inline-block px-6 py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] text-white font-medium rounded-[var(--radius-md)] text-sm transition-colors"
                >
                    Sign in
                </a>
            </div>
        {:else}
            <div class="text-center mb-8">
                <h1 class="text-2xl font-bold text-white">Set new password</h1>
            </div>
            <form onsubmit={handleSubmit} class="space-y-4">
                {#if error}
                    <div
                        class="px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
                    >
                        {error}
                    </div>
                {/if}
                <div>
                    <label
                        for="password"
                        class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                        >New password</label
                    >
                    <input
                        id="password"
                        type="password"
                        bind:value={password}
                        required
                        minlength="8"
                        autocomplete="new-password"
                        class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
                        placeholder="Min. 8 characters"
                    />
                </div>
                <div>
                    <label
                        for="password2"
                        class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                        >Confirm new password</label
                    >
                    <input
                        id="password2"
                        type="password"
                        bind:value={password2}
                        required
                        autocomplete="new-password"
                        class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
                        placeholder="••••••••"
                    />
                </div>
                <button
                    type="submit"
                    disabled={loading}
                    class="w-full py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white font-medium rounded-[var(--radius-md)] transition-colors text-sm"
                >
                    {loading ? "Updating…" : "Update password"}
                </button>
            </form>
        {/if}
    </div>
</div>

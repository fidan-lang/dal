<script lang="ts">
    import { auth, DalApiError } from "$lib/api";

    let email = $state("");
    let error = $state("");
    let success = $state(false);
    let loading = $state(false);

    async function handleSubmit(e: Event) {
        e.preventDefault();
        error = "";
        loading = true;
        try {
            await auth.forgotPassword(fetch, email);
            success = true;
        } catch (err) {
            error =
                err instanceof DalApiError
                    ? err.message
                    : "Something went wrong. Please try again.";
        } finally {
            loading = false;
        }
    }
</script>

<svelte:head>
    <title>Forgot password — Dal</title>
</svelte:head>

<div
    class="min-h-[calc(100vh-7rem)] flex items-center justify-center px-4 py-12"
>
    <div class="w-full max-w-sm">
        {#if success}
            <div class="text-center">
                <h1 class="text-2xl font-bold text-white mb-2">
                    Check your email
                </h1>
                <p class="text-[var(--color-text-muted)] text-sm">
                    If an account exists for <strong
                        class="text-[var(--color-text)]">{email}</strong
                    >, we sent a password reset link.
                </p>
                <a
                    href="/login"
                    class="mt-6 inline-block text-sm text-[var(--color-primary)] hover:underline"
                    >Back to sign in</a
                >
            </div>
        {:else}
            <div class="text-center mb-8">
                <h1 class="text-2xl font-bold text-white">
                    Reset your password
                </h1>
                <p class="text-[var(--color-text-muted)] text-sm mt-2">
                    Enter your email and we'll send you a reset link.
                </p>
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
                        for="email"
                        class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                        >Email</label
                    >
                    <input
                        id="email"
                        type="email"
                        bind:value={email}
                        required
                        class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
                        placeholder="you@example.com"
                    />
                </div>
                <button
                    type="submit"
                    disabled={loading}
                    class="w-full py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white font-medium rounded-[var(--radius-md)] transition-colors text-sm"
                >
                    {loading ? "Sending…" : "Send reset link"}
                </button>
            </form>
            <p class="text-center text-sm text-[var(--color-text-muted)] mt-6">
                <a
                    href="/login"
                    class="text-[var(--color-primary)] hover:underline"
                    >Back to sign in</a
                >
            </p>
        {/if}
    </div>
</div>

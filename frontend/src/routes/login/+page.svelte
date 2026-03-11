<script lang="ts">
    import { goto } from "$app/navigation";
    import { auth, DalApiError } from "$lib/api";
    import { currentUser } from "$lib/stores/auth";

    let username = $state("");
    let password = $state("");
    let error = $state("");
    let loading = $state(false);

    async function handleSubmit(e: Event) {
        e.preventDefault();
        error = "";
        loading = true;
        try {
            const user = await auth.login(fetch, username, password);
            currentUser.set(user);
            goto("/dashboard");
        } catch (err) {
            error =
                err instanceof DalApiError
                    ? err.message
                    : "Login failed. Please try again.";
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
                    for="username"
                    class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                    >Username</label
                >
                <input
                    id="username"
                    type="text"
                    bind:value={username}
                    required
                    autocomplete="username"
                    class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
                    placeholder="your-username"
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
            <a
                href="/register"
                class="text-[var(--color-primary)] hover:underline">Sign up</a
            >
        </p>
    </div>
</div>

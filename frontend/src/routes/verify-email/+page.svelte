<script lang="ts">
    let { data } = $props();
</script>

<svelte:head>
    <title>Verify email — Dal</title>
</svelte:head>

<div
    class="min-h-[calc(100vh-7rem)] flex items-center justify-center px-4 py-12"
>
    <div class="w-full max-w-sm text-center">
        {#if data.status === "ok"}
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
            <h1 class="text-2xl font-bold text-white mb-2">Email verified!</h1>
            <p class="text-[var(--color-text-muted)] text-sm mb-6">
                Your account is now active. You can sign in.
            </p>
            <a
                href="/login"
                class="inline-block px-6 py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] text-white font-medium rounded-[var(--radius-md)] transition-colors text-sm"
            >
                Sign in
            </a>
        {:else if data.status === "missing"}
            <h1 class="text-2xl font-bold text-white mb-2">Missing token</h1>
            <p class="text-[var(--color-text-muted)] text-sm">
                No verification token was provided. Please use the link in your
                email.
            </p>
        {:else}
            <div
                class="w-16 h-16 bg-[var(--color-danger)]/10 rounded-full flex items-center justify-center mx-auto mb-4"
            >
                <svg
                    class="w-8 h-8 text-[var(--color-danger)]"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M6 18L18 6M6 6l12 12"
                    />
                </svg>
            </div>
            <h1 class="text-2xl font-bold text-white mb-2">
                Invalid or expired link
            </h1>
            <p class="text-[var(--color-text-muted)] text-sm mb-6">
                This verification link is no longer valid. Try registering
                again.
            </p>
            <a
                href="/register"
                class="text-sm text-[var(--color-primary)] hover:underline"
                >Back to registration</a
            >
        {/if}
    </div>
</div>

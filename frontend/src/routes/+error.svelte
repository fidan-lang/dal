<script lang="ts">
    import { page } from "$app/state";
</script>

<svelte:head>
    <title>{page.status} — Dal</title>
</svelte:head>

<div
    class="min-h-[calc(100vh-7rem)] flex items-center justify-center px-4 py-12 text-center"
>
    <div>
        <div class="text-6xl font-bold text-[var(--color-primary)] mb-4">
            {page.status}
        </div>
        <h1 class="text-xl font-semibold text-white mb-2">
            {page.error?.message ?? "Something went wrong"}
        </h1>
        <p class="text-[var(--color-text-muted)] text-sm mb-8">
            {#if page.status === 404}
                The page you're looking for doesn't exist.
            {:else}
                An unexpected error occurred.
            {/if}
        </p>
        <a
            href="/"
            class="px-6 py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] text-white text-sm font-medium rounded-[var(--radius-md)] transition-colors"
        >
            Go home
        </a>
    </div>
</div>

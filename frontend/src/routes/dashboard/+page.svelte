<script lang="ts">
    import PackageCard from "$lib/components/PackageCard.svelte";
    import { currentUser } from "$lib/stores/auth";
    import { formatNumber } from "$lib/utils";

    let { data } = $props();
    let totalDownloads = $derived(
        data.packages.items.reduce((a, p) => a + p.downloads, 0),
    );
</script>

<svelte:head>
    <title>Dashboard — Dal</title>
</svelte:head>

<div class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
    <div class="flex items-start justify-between mb-8">
        <div>
            <h1 class="text-2xl font-bold text-white">Dashboard</h1>
            {#if $currentUser}
                <p class="text-[var(--color-text-muted)] text-sm mt-1">
                    Welcome back, <span class="text-[var(--color-text)]"
                        >{$currentUser.display_name ??
                            $currentUser.username}</span
                    >
                    {#if !$currentUser.email_verified}
                        <span
                            class="ml-2 text-xs text-[var(--color-warning)] bg-[var(--color-warning)]/10 border border-[var(--color-warning)]/30 px-2 py-0.5 rounded"
                            >email unverified</span
                        >
                    {/if}
                </p>
            {/if}
        </div>
    </div>

    <!-- Summary cards -->
    <div class="grid grid-cols-1 sm:grid-cols-3 gap-4 mb-10">
        <div
            class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
            <div class="text-2xl font-bold text-white">
                {data.packages.total}
            </div>
            <div class="text-sm text-[var(--color-text-muted)] mt-1">
                Packages
            </div>
        </div>
        <div
            class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
            <div class="text-2xl font-bold text-white">
                {formatNumber(totalDownloads)}
            </div>
            <div class="text-sm text-[var(--color-text-muted)] mt-1">
                Total downloads
            </div>
        </div>
        <div
            class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
            <a href="/settings/tokens" class="block">
                <div class="text-2xl font-bold text-[var(--color-primary)]">
                    →
                </div>
                <div class="text-sm text-[var(--color-text-muted)] mt-1">
                    API Tokens
                </div>
            </a>
        </div>
    </div>

    <!-- Package list -->
    <h2 class="text-lg font-semibold text-white mb-4">Your packages</h2>
    {#if data.packages.items.length}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {#each data.packages.items as pkg}
                <PackageCard {pkg} />
            {/each}
        </div>
    {:else}
        <div class="text-center py-16 text-[var(--color-text-muted)]">
            <p>You haven't published any packages yet.</p>
            <p class="text-sm mt-2">
                Run <code class="font-mono text-[var(--color-primary-light)]"
                    >fidan dal publish</code
                > in your project to get started.
            </p>
        </div>
    {/if}
</div>

<script lang="ts">
    import { goto } from "$app/navigation";
    import { page } from "$app/state";
    import PackageCard from "$lib/components/PackageCard.svelte";
    import Pagination from "$lib/components/Pagination.svelte";
    import { formatNumber } from "$lib/utils";

    let { data } = $props();

    function changePage(p: number) {
        const u = new URL(page.url);
        u.searchParams.set("page", String(p));
        goto(u.toString());
    }
</script>

<svelte:head>
    <title>All packages — Dal</title>
    <meta
        name="description"
        content="Browse all packages in the Dal Fidan registry."
    />
</svelte:head>

<div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-12">
    <div class="flex items-center justify-between mb-8">
        <div>
            <h1 class="text-2xl font-bold text-white">All packages</h1>
            <p class="text-[var(--color-text-muted)] text-sm mt-1">
                {formatNumber(data.total)} packages
            </p>
        </div>
        <a
            href="/search"
            class="text-sm text-[var(--color-primary)] hover:underline"
            >Search packages →</a
        >
    </div>

    {#if data.items.length}
        <div
            class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4 mb-10"
        >
            {#each data.items as pkg}
                <PackageCard {pkg} />
            {/each}
        </div>
        <div class="flex justify-center">
            <Pagination
                total={data.total}
                page={data.page}
                pages={data.pages}
                onchange={changePage}
            />
        </div>
    {:else}
        <div class="text-center py-20 text-[var(--color-text-muted)]">
            <p class="text-lg">No packages yet.</p>
            <p class="text-sm mt-2">
                Be the first to <a
                    href="/register"
                    class="text-[var(--color-primary)] hover:underline"
                    >publish</a
                >!
            </p>
        </div>
    {/if}
</div>

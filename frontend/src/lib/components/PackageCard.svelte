<script lang="ts">
    import type { Package } from "$lib/api";
    import { formatNumber, timeAgo, truncate } from "$lib/utils";

    let { pkg }: { pkg: Package } = $props();
</script>

<a
    href="/packages/{pkg.name}"
    class="block p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)] hover:border-[var(--color-primary)] hover:-translate-y-1 hover:shadow-[0_8px_30px_rgba(132,204,22,0.1)] transition-all duration-200 group"
>
    <div class="flex items-start justify-between gap-4 mb-2">
        <h3
            class="font-semibold text-white group-hover:text-[var(--color-primary)] transition-colors font-mono"
        >
            {pkg.name}
        </h3>
        {#if pkg.latest_version}
            <span
                class="shrink-0 text-xs text-[var(--color-primary)] bg-[var(--color-primary)]/10 px-2 py-0.5 rounded-full font-mono"
            >
                v{pkg.latest_version}
            </span>
        {/if}
    </div>

    {#if pkg.description}
        <p class="text-sm text-[var(--color-text-muted)] mb-3 leading-relaxed">
            {truncate(pkg.description, 120)}
        </p>
    {/if}

    {#if pkg.keywords?.length}
        <div class="flex flex-wrap gap-1.5 mb-3">
            {#each pkg.keywords.slice(0, 5) as kw}
                <span
                    class="text-xs text-[var(--color-text-muted)] bg-[var(--color-surface-3)] px-2 py-0.5 rounded border border-[var(--color-border)]"
                >
                    {kw}
                </span>
            {/each}
        </div>
    {/if}

    <div class="flex items-center gap-4 text-xs text-[var(--color-text-muted)]">
        <span class="flex items-center gap-1">
            <svg
                class="w-3.5 h-3.5"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
            >
                <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4"
                />
            </svg>
            {formatNumber(pkg.downloads)}
        </span>
        <span>{timeAgo(pkg.updated_at)}</span>
        {#if pkg.license}
            <span>{pkg.license}</span>
        {/if}
    </div>
</a>

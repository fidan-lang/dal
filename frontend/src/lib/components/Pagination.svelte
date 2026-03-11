<script lang="ts">
    interface Props {
        total: number;
        page: number;
        pages: number;
        onchange: (page: number) => void;
    }
    let { total, page, pages, onchange }: Props = $props();

    function range(from: number, to: number) {
        return Array.from({ length: to - from + 1 }, (_, i) => from + i);
    }

    let visiblePages = $derived(() => {
        if (pages <= 7) return range(1, pages);
        if (page <= 4) return [...range(1, 5), -1, pages];
        if (page >= pages - 3) return [1, -1, ...range(pages - 4, pages)];
        return [1, -1, ...range(page - 1, page + 1), -1, pages];
    });
</script>

{#if pages > 1}
    <nav class="flex items-center gap-1" aria-label="Pagination">
        <button
            onclick={() => onchange(page - 1)}
            disabled={page === 1}
            class="px-3 py-1.5 text-sm rounded-[var(--radius-sm)] border border-[var(--color-border)] text-[var(--color-text-muted)] hover:text-[var(--color-text)] hover:border-[var(--color-primary)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            aria-label="Previous page"
        >
            ←
        </button>

        {#each visiblePages() as p}
            {#if p === -1}
                <span class="px-2 text-[var(--color-text-muted)]">…</span>
            {:else}
                <button
                    onclick={() => onchange(p)}
                    class="px-3 py-1.5 text-sm rounded-[var(--radius-sm)] border transition-colors {p ===
                    page
                        ? 'border-[var(--color-primary)] text-[var(--color-primary)] bg-[var(--color-primary)]/10'
                        : 'border-[var(--color-border)] text-[var(--color-text-muted)]'}"
                    aria-current={p === page ? "page" : undefined}
                >
                    {p}
                </button>
            {/if}
        {/each}

        <button
            onclick={() => onchange(page + 1)}
            disabled={page === pages}
            class="px-3 py-1.5 text-sm rounded-[var(--radius-sm)] border border-[var(--color-border)] text-[var(--color-text-muted)] hover:text-[var(--color-text)] hover:border-[var(--color-primary)] disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            aria-label="Next page"
        >
            →
        </button>
    </nav>
{/if}

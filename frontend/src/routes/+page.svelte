<script lang="ts">
    import PackageCard from "$lib/components/PackageCard.svelte";
    import { formatNumber } from "$lib/utils";

    let { data } = $props();
</script>

<svelte:head>
    <title>Dal — The Fidan Package Registry</title>
    <meta
        name="description"
        content="Discover and publish packages for the Fidan programming language."
    />
</svelte:head>

<!-- Hero -->
<section class="relative overflow-hidden">
    <!-- Gradient backdrop -->
    <div
        class="pointer-events-none absolute inset-0 bg-gradient-to-b from-[var(--color-primary)]/8 to-transparent"
        aria-hidden="true"
    ></div>

    <div
        class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-24 text-center relative"
    >
        <div
            class="inline-flex items-center gap-2 px-3 py-1 bg-[var(--color-primary)]/10 border border-[var(--color-primary)]/30 rounded-full text-sm text-[var(--color-primary)] mb-6 animate-fade-in-up"
        >
            <span class="w-1.5 h-1.5 rounded-full bg-[var(--color-primary)]"
            ></span>
            For the Fidan programming language
        </div>
        <h1
            class="text-4xl sm:text-5xl lg:text-6xl font-bold text-white mb-6 leading-tight tracking-tight animate-fade-in-up [animation-delay:75ms]"
        >
            The package registry for<br />
            <span class="text-[var(--color-primary)]">Fidan</span>
        </h1>
        <p
            class="text-lg text-[var(--color-text-muted)] max-w-2xl mx-auto mb-10 animate-fade-in-up [animation-delay:150ms]"
        >
            Discover, publish, and manage packages for the Fidan programming
            language. Install with <code
                class="font-mono text-[var(--color-primary-light)]"
                >fidan dal add &lt;package&gt;</code
            >.
        </p>

        <!-- Search bar -->
        <form
            action="/search"
            method="GET"
            class="max-w-xl mx-auto flex gap-2 animate-fade-in-up [animation-delay:225ms]"
        >
            <input
                name="q"
                type="search"
                placeholder="Search packages…"
                class="flex-1 px-4 py-3 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none focus:ring-2 focus:ring-[var(--color-primary)]/20 focus:shadow-[0_0_20px_rgba(132,204,22,0.1)] transition-all duration-200 text-sm"
            />
            <button
                type="submit"
                class="px-6 py-3 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] active:scale-95 text-white font-medium rounded-[var(--radius-md)] transition-all duration-200 text-sm"
            >
                Search
            </button>
        </form>

        <!-- Quick-start snippet -->
        <div
            class="mt-10 inline-block text-left animate-fade-in-up [animation-delay:300ms]"
        >
            <p class="text-xs text-[var(--color-text-muted)] mb-2 text-center">
                Quick install
            </p>
            <div
                class="flex items-center gap-3 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)] px-5 py-3 font-mono text-sm"
            >
                <span class="text-[var(--color-text-muted)] select-none">$</span
                >
                <span>
                    <span class="text-[var(--color-primary-light)]">fidan</span>
                    <span class="text-[var(--color-primary-light)]">dal</span>
                    <span class="text-[var(--color-text)]">add</span>
                    <span class="text-[var(--color-warning)]">my-package</span>
                </span>
            </div>
        </div>
    </div>
</section>

<!-- Stats bar -->
<section
    class="border-y border-[var(--color-border)] bg-[var(--color-surface-2)]"
>
    <div
        class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6 grid grid-cols-3 gap-4 text-center"
    >
        <div>
            <div class="text-2xl font-bold text-white">
                {formatNumber(data.recent.total)}
            </div>
            <div class="text-sm text-[var(--color-text-muted)]">Packages</div>
        </div>
        <div>
            <div class="text-2xl font-bold text-white">∞</div>
            <div class="text-sm text-[var(--color-text-muted)]">Versions</div>
        </div>
        <div>
            <div class="text-2xl font-bold text-white">Free</div>
            <div class="text-sm text-[var(--color-text-muted)]">Always</div>
        </div>
    </div>
</section>

<!-- Recent packages -->
<section class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-16">
    <div class="flex items-center justify-between mb-8">
        <h2 class="text-xl font-semibold text-white">Recently updated</h2>
        <a
            href="/packages"
            class="text-sm text-[var(--color-primary)] hover:text-[var(--color-primary-light)] transition-colors duration-200 flex items-center gap-1 group/link"
        >
            View all
            <span
                class="transition-transform duration-200 group-hover/link:translate-x-1"
                >→</span
            >
        </a>
    </div>
    {#if data.recent.items.length}
        <div
            class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4"
        >
            {#each data.recent.items as pkg}
                <PackageCard {pkg} />
            {/each}
        </div>
    {:else}
        <p class="text-[var(--color-text-muted)] text-sm">
            No packages yet. Be the first to publish!
        </p>
    {/if}
</section>

<!-- CTA / feature cards -->
<section class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 pb-20">
    <div
        class="grid grid-cols-1 md:grid-cols-3 gap-6 [&>div]:transition-all [&>div]:duration-200 [&>div:hover]:-translate-y-1 [&>div:hover]:border-[#84cc16]/40 [&>div:hover]:shadow-[0_8px_30px_rgba(132,204,22,0.08)]"
    >
        <div
            class="p-6 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
            <div
                class="w-10 h-10 bg-[var(--color-primary)]/15 rounded-[var(--radius-md)] flex items-center justify-center mb-4"
            >
                <svg
                    class="w-5 h-5 text-[var(--color-primary)]"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M9 19l3 3m0 0l3-3m-3 3V10"
                    />
                </svg>
            </div>
            <h3 class="font-semibold text-white mb-2">Publish instantly</h3>
            <p class="text-sm text-[var(--color-text-muted)]">
                Tag a release, run <code
                    class="font-mono text-xs text-[var(--color-primary-light)]"
                    >dal publish</code
                >, and your package is live in seconds.
            </p>
        </div>
        <div
            class="p-6 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
            <div
                class="w-10 h-10 bg-[var(--color-primary)]/15 rounded-[var(--radius-md)] flex items-center justify-center mb-4"
            >
                <svg
                    class="w-5 h-5 text-[var(--color-primary)]"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0z"
                    />
                </svg>
            </div>
            <h3 class="font-semibold text-white mb-2">Collaborate</h3>
            <p class="text-sm text-[var(--color-text-muted)]">
                Add co-owners to your packages and collaborate with the
                community.
            </p>
        </div>
        <div
            class="p-6 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
            <div
                class="w-10 h-10 bg-[var(--color-primary)]/15 rounded-[var(--radius-md)] flex items-center justify-center mb-4"
            >
                <svg
                    class="w-5 h-5 text-[var(--color-primary)]"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                >
                    <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"
                    />
                </svg>
            </div>
            <h3 class="font-semibold text-white mb-2">Secure by default</h3>
            <p class="text-sm text-[var(--color-text-muted)]">
                All uploads are verified. Archive bombs, path traversal, and
                oversized packages are rejected automatically.
            </p>
        </div>
    </div>
</section>

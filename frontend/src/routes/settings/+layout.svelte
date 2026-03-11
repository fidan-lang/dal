<script lang="ts">
    import { page } from "$app/state";
    let { children } = $props();

    const navLinks = [
        { label: "Profile", href: "/settings" },
        { label: "API Tokens", href: "/settings/tokens" },
    ];
</script>

<div class="max-w-4xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
    <h1 class="text-2xl font-bold text-white mb-8">Settings</h1>
    <div class="flex flex-col sm:flex-row gap-8">
        <nav class="sm:w-44 shrink-0">
            <ul class="space-y-1">
                {#each navLinks as link}
                    <li>
                        <a
                            href={link.href}
                            class="block px-3 py-2 text-sm rounded-[var(--radius-sm)] transition-colors"
                            class:bg-[var(--color-surface-3)]={page.url
                                .pathname === link.href}
                            class:text-white={page.url.pathname === link.href}
                            class:text-[var(--color-text-muted)]={page.url
                                .pathname !== link.href}
                            class:hover:text-[var(--color-text)]={page.url
                                .pathname !== link.href}
                        >
                            {link.label}
                        </a>
                    </li>
                {/each}
            </ul>
        </nav>
        <div class="flex-1 min-w-0">
            {@render children()}
        </div>
    </div>
</div>

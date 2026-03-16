<script lang="ts">
    import { goto } from "$app/navigation";
    import { page } from "$app/state";
    import { auth } from "$lib/api";
    import { currentUser } from "$lib/stores/auth";
    import { slide } from "svelte/transition";

    let mobileOpen = $state(false);
    let dropdownOpen = $state(false);

    async function handleLogout() {
        await auth.logout(fetch);
        currentUser.set(null);
        goto("/");
    }

    const navLinks = [
        { label: "Packages", href: "/packages" },
        { label: "Search", href: "/search" },
    ];

    function handleWindowClick(e: MouseEvent) {
        if (dropdownOpen && !(e.target as Element).closest?.(".relative")) {
            dropdownOpen = false;
        }
    }
</script>

<nav class="border-b border-[var(--color-border)] bg-[var(--color-surface-2)]">
    <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div class="relative flex items-center h-14">
            <!-- Logo -->
            <a
                href="/"
                class="flex items-center gap-2 font-bold text-lg text-white no-underline group"
            >
                <img
                    src="/favicon.svg"
                    width="40"
                    height="40"
                    alt=""
                    aria-hidden="true"
                    class="transition-transform duration-200 group-hover:scale-110"
                />
                <span
                    class="transition-colors duration-200 group-hover:text-[var(--color-primary-light)]"
                    >dal</span
                >
            </a>

            <!-- Desktop nav -->
            <div
                class="hidden md:flex absolute left-1/2 -translate-x-1/2 items-center gap-6"
            >
                {#each navLinks as link}
                    <a
                        href={link.href}
                        class="relative text-sm text-[var(--color-text-muted)] hover:text-white transition-colors duration-200 pb-1 after:content-[''] after:absolute after:bottom-0 after:left-0 after:h-[2px] after:w-0 hover:after:w-full after:bg-[var(--color-primary)] after:transition-[width] after:duration-200"
                        class:text-white={page.url.pathname.startsWith(
                            link.href,
                        )}
                    >
                        {link.label}
                    </a>
                {/each}
            </div>

            <!-- Auth area -->
            <div class="hidden md:flex ml-auto items-center gap-3">
                {#if $currentUser}
                    <div class="relative">
                        <button
                            onclick={() => (dropdownOpen = !dropdownOpen)}
                            class="flex items-center gap-2 px-3 py-1.5 rounded-[var(--radius-sm)] text-sm text-[var(--color-text)] hover:bg-[var(--color-surface-3)] transition-colors"
                        >
                            <span
                                class="w-7 h-7 rounded-full bg-[var(--color-primary)] text-white text-xs font-bold flex items-center justify-center uppercase"
                            >
                                {$currentUser.username[0]}
                            </span>
                            <span>{$currentUser.username}</span>
                            {#if $currentUser.is_admin}
                                <span
                                    class="px-2 py-0.5 rounded-full border border-[var(--color-primary)]/30 bg-[var(--color-primary)]/10 text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--color-primary-light)]"
                                >
                                    Admin
                                </span>
                            {/if}
                            <svg
                                class="w-4 h-4 text-[var(--color-text-muted)]"
                                fill="none"
                                stroke="currentColor"
                                viewBox="0 0 24 24"
                            >
                                <path
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    stroke-width="2"
                                    d="M19 9l-7 7-7-7"
                                />
                            </svg>
                        </button>
                        {#if dropdownOpen}
                            <div
                                class="absolute right-0 top-full mt-1 w-48 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)] shadow-xl z-50 py-1 animate-scale-in"
                                role="menu"
                            >
                                <a
                                    href="/dashboard"
                                    class="block px-4 py-2 text-sm text-[var(--color-text)] hover:bg-[var(--color-surface-3)]"
                                    role="menuitem">Dashboard</a
                                >
                                <a
                                    href="/settings"
                                    class="block px-4 py-2 text-sm text-[var(--color-text)] hover:bg-[var(--color-surface-3)]"
                                    role="menuitem">Settings</a
                                >
                                <a
                                    href="/settings/tokens"
                                    class="block px-4 py-2 text-sm text-[var(--color-text)] hover:bg-[var(--color-surface-3)]"
                                    role="menuitem">API Tokens</a
                                >
                                {#if $currentUser.is_admin}
                                    <div
                                        class="px-4 py-2 text-xs text-[var(--color-primary-light)] bg-[var(--color-primary)]/5 border-y border-[var(--color-primary)]/10"
                                    >
                                        Platform admin access enabled
                                    </div>
                                {/if}
                                <hr class="border-[var(--color-border)] my-1" />
                                <button
                                    onclick={handleLogout}
                                    class="block w-full text-left px-4 py-2 text-sm text-[var(--color-danger)] hover:bg-[var(--color-surface-3)]"
                                    role="menuitem"
                                >
                                    Sign out
                                </button>
                            </div>
                        {/if}
                    </div>
                {:else}
                    <a
                        href="/login"
                        class="px-3 py-1.5 text-sm text-[var(--color-text-muted)] hover:text-white transition-colors duration-200"
                    >
                        Sign in
                    </a>
                    <a
                        href="/register"
                        class="px-4 py-1.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] active:scale-95 text-white text-sm font-medium rounded-[var(--radius-sm)] transition-all duration-200"
                    >
                        Sign up
                    </a>
                {/if}
            </div>

            <!-- Mobile menu button -->
            <button
                class="md:hidden p-2 text-[var(--color-text-muted)] hover:text-[var(--color-text)] transition-colors duration-200"
                onclick={() => (mobileOpen = !mobileOpen)}
                aria-label="Toggle menu"
                aria-expanded={mobileOpen}
            >
                <svg
                    class="w-5 h-5"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                >
                    <!-- top bar: slides down + rotates into first diagonal -->
                    <line
                        x1="4"
                        y1="6"
                        x2="20"
                        y2="6"
                        stroke-linecap="round"
                        stroke-width="2"
                        style="
                            transform-origin: 12px 6px;
                            transition: transform 300ms cubic-bezier(0.4,0,0.2,1);
                            transform: {mobileOpen
                            ? 'translateY(6px) rotate(45deg)'
                            : 'none'};
                        "
                    />
                    <!-- middle bar: fades out -->
                    <line
                        x1="4"
                        y1="12"
                        x2="20"
                        y2="12"
                        stroke-linecap="round"
                        stroke-width="2"
                        style="
                            transition: opacity 200ms ease;
                            opacity: {mobileOpen ? 0 : 1};
                        "
                    />
                    <!-- bottom bar: slides up + rotates into second diagonal -->
                    <line
                        x1="4"
                        y1="18"
                        x2="20"
                        y2="18"
                        stroke-linecap="round"
                        stroke-width="2"
                        style="
                            transform-origin: 12px 18px;
                            transition: transform 300ms cubic-bezier(0.4,0,0.2,1);
                            transform: {mobileOpen
                            ? 'translateY(-6px) rotate(-45deg)'
                            : 'none'};
                        "
                    />
                </svg>
            </button>
        </div>
    </div>

    <!-- Mobile menu -->
    {#if mobileOpen}
        <div
            transition:slide={{ duration: 250, axis: "y" }}
            class="md:hidden border-t border-[var(--color-border)] bg-[var(--color-surface-2)] px-4 py-3 flex flex-col gap-3 overflow-hidden"
        >
            {#each navLinks as link}
                <a
                    href={link.href}
                    class="text-sm text-[var(--color-text-muted)]"
                    >{link.label}</a
                >
            {/each}
            {#if $currentUser}
                <div class="flex items-center gap-2">
                    <a
                        href="/dashboard"
                        class="text-sm text-[var(--color-text-muted)]">Dashboard</a
                    >
                    {#if $currentUser.is_admin}
                        <span
                            class="px-2 py-0.5 rounded-full border border-[var(--color-primary)]/30 bg-[var(--color-primary)]/10 text-[10px] font-semibold uppercase tracking-[0.08em] text-[var(--color-primary-light)]"
                        >
                            Admin
                        </span>
                    {/if}
                </div>
                <a
                    href="/settings"
                    class="text-sm text-[var(--color-text-muted)]">Settings</a
                >
                <button
                    onclick={handleLogout}
                    class="text-left text-sm text-[var(--color-danger)]"
                    >Sign out</button
                >
            {:else}
                <a href="/login" class="text-sm text-[var(--color-text-muted)]"
                    >Sign in</a
                >
                <a href="/register" class="text-sm text-[var(--color-primary)]"
                    >Sign up</a
                >
            {/if}
        </div>
    {/if}
</nav>

<!-- Close dropdown on outside click -->
<svelte:window onclick={handleWindowClick} />

<script lang="ts">
    import type { PackageVersion } from "$lib/api";
    import { DalApiError, owners, versions as versionsApi } from "$lib/api";
    import { currentUser } from "$lib/stores/auth";
    import {
        formatBytes,
        formatNumber,
        renderMarkdown,
        timeAgo,
    } from "$lib/utils";
    import { onMount } from "svelte";

    let { data } = $props();

    let tab = $state<"readme" | "versions" | "owners">("readme");
    let readme = $state("");
    let ownersList = $state<Awaited<ReturnType<typeof owners.list>>>([]);
    let yankError = $state("");

    // Latest non-yanked version's README
    let latestVersion = $derived(
        data.versions.find((v: PackageVersion) => !v.yanked) ??
            data.versions[0],
    );

    onMount(async () => {
        if (latestVersion?.readme) {
            readme = renderMarkdown(latestVersion.readme);
        }
        try {
            ownersList = await owners.list(fetch, data.pkg.name);
        } catch {
            // non-critical
        }
    });

    async function yank(version: string) {
        yankError = "";
        try {
            await versionsApi.yank(fetch, data.pkg.name, version);
            // reload
            window.location.reload();
        } catch (err) {
            yankError =
                err instanceof DalApiError
                    ? err.message
                    : "Failed to yank version.";
        }
    }

    async function unyank(version: string) {
        yankError = "";
        try {
            await versionsApi.unyank(fetch, data.pkg.name, version);
            window.location.reload();
        } catch (err) {
            yankError =
                err instanceof DalApiError
                    ? err.message
                    : "Failed to unyank version.";
        }
    }

    let isOwner = $derived(
        !!$currentUser &&
            ownersList.some((o) => o.username === $currentUser?.username),
    );
</script>

<svelte:head>
    <title>{data.pkg.name} — Dal</title>
    <meta
        name="description"
        content={data.pkg.description ?? `${data.pkg.name} Fidan package`}
    />
</svelte:head>

<div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
    <div class="flex flex-col lg:flex-row gap-10">
        <!-- Main content -->
        <div class="flex-1 min-w-0">
            <!-- Header -->
            <div class="mb-6">
                <div class="flex items-start justify-between gap-4 flex-wrap">
                    <div>
                        <h1 class="text-3xl font-bold text-white font-mono">
                            {data.pkg.name}
                        </h1>
                        {#if data.pkg.description}
                            <p class="text-[var(--color-text-muted)] mt-2">
                                {data.pkg.description}
                            </p>
                        {/if}
                    </div>
                    {#if latestVersion}
                        <span
                            class="shrink-0 text-sm font-mono text-[var(--color-primary)] bg-[var(--color-primary)]/10 border border-[var(--color-primary)]/30 px-3 py-1 rounded-full"
                        >
                            v{latestVersion.version}
                        </span>
                    {/if}
                </div>

                {#if data.pkg.keywords?.length}
                    <div class="flex flex-wrap gap-1.5 mt-4">
                        {#each data.pkg.keywords as kw}
                            <a
                                href="/search?q={encodeURIComponent(kw)}"
                                class="text-xs text-[var(--color-text-muted)] bg-[var(--color-surface-3)] px-2 py-0.5 rounded border border-[var(--color-border)] hover:border-[var(--color-primary)] transition-colors"
                            >
                                {kw}
                            </a>
                        {/each}
                    </div>
                {/if}
            </div>

            <!-- Install snippet -->
            {#if latestVersion}
                <div
                    class="mb-6 p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
                >
                    <p class="text-xs text-[var(--color-text-muted)] mb-2">
                        Install
                    </p>
                    <code
                        class="font-mono text-sm text-[var(--color-primary-light)]"
                    >
                        fidan dal add {data.pkg.name}
                    </code>
                </div>
            {/if}

            <!-- Tabs -->
            <div class="border-b border-[var(--color-border)] mb-6 flex gap-6">
                {#each [["readme", "Readme"], ["versions", "Versions"], ["owners", "Owners"]] as [id, label]}
                    <button
                        onclick={() => {
                            tab = id as "readme" | "versions" | "owners";
                        }}
                        class="pb-3 text-sm border-b-2 transition-colors -mb-px"
                        class:border-[var(--color-primary)]={tab === id}
                        class:text-white={tab === id}
                        class:border-transparent={tab !== id}
                        class:text-[var(--color-text-muted)]={tab !== id}
                    >
                        {label}
                    </button>
                {/each}
            </div>

            <!-- Tab content -->
            {#if tab === "readme"}
                {#if readme}
                    <div class="prose">{@html readme}</div>
                {:else}
                    <p class="text-[var(--color-text-muted)] text-sm">
                        No readme provided.
                    </p>
                {/if}
            {:else if tab === "versions"}
                {#if yankError}
                    <div
                        class="mb-4 px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
                    >
                        {yankError}
                    </div>
                {/if}
                <div class="space-y-2">
                    {#each data.versions as v}
                        <div
                            class="flex items-center justify-between p-3 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
                        >
                            <div class="flex items-center gap-3">
                                <a
                                    href="/packages/{data.pkg
                                        .name}/versions/{v.version}"
                                    class="font-mono text-sm text-[var(--color-primary)] hover:underline"
                                >
                                    v{v.version}
                                </a>
                                {#if v.yanked}
                                    <span
                                        class="text-xs text-[var(--color-warning)] bg-[var(--color-warning)]/10 border border-[var(--color-warning)]/30 px-2 py-0.5 rounded"
                                        >yanked</span
                                    >
                                {/if}
                            </div>
                            <div
                                class="flex items-center gap-4 text-xs text-[var(--color-text-muted)]"
                            >
                                <span>{formatBytes(v.size_bytes)}</span>
                                <span>{timeAgo(v.published_at)}</span>
                                {#if isOwner}
                                    {#if v.yanked}
                                        <button
                                            onclick={() => unyank(v.version)}
                                            class="text-[var(--color-primary)] hover:underline"
                                            >Unyank</button
                                        >
                                    {:else}
                                        <button
                                            onclick={() => yank(v.version)}
                                            class="text-[var(--color-danger)] hover:underline"
                                            >Yank</button
                                        >
                                    {/if}
                                {/if}
                            </div>
                        </div>
                    {/each}
                </div>
            {:else if tab === "owners"}
                <div class="space-y-3">
                    {#each ownersList as owner}
                        <div
                            class="flex items-center gap-3 p-3 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
                        >
                            <div
                                class="w-8 h-8 rounded-full bg-[var(--color-primary)] text-white text-xs font-bold flex items-center justify-center uppercase shrink-0"
                            >
                                {owner.username[0]}
                            </div>
                            <div>
                                <a
                                    href="/users/{owner.username}"
                                    class="text-sm text-[var(--color-text)] hover:text-[var(--color-primary)]"
                                >
                                    {owner.display_name ?? owner.username}
                                </a>
                                <span
                                    class="text-xs text-[var(--color-text-muted)] ml-2"
                                    >{owner.role}</span
                                >
                            </div>
                        </div>
                    {/each}
                    {#if isOwner}
                        <a
                            href="/dashboard/packages/{data.pkg.name}/owners"
                            class="text-sm text-[var(--color-primary)] hover:underline"
                        >
                            Manage owners →
                        </a>
                    {/if}
                </div>
            {/if}
        </div>

        <!-- Sidebar -->
        <aside class="lg:w-64 shrink-0 space-y-6">
            <div
                class="p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)] space-y-3 text-sm"
            >
                <div class="flex justify-between">
                    <span class="text-[var(--color-text-muted)]">Downloads</span
                    >
                    <span class="text-white font-medium"
                        >{formatNumber(data.pkg.downloads)}</span
                    >
                </div>
                {#if data.pkg.license}
                    <div class="flex justify-between">
                        <span class="text-[var(--color-text-muted)]"
                            >License</span
                        >
                        <span class="text-white">{data.pkg.license}</span>
                    </div>
                {/if}
                {#if data.pkg.homepage_url}
                    <div class="flex justify-between gap-2 min-w-0">
                        <span class="text-[var(--color-text-muted)] shrink-0"
                            >Homepage</span
                        >
                        <a
                            href={data.pkg.homepage_url}
                            target="_blank"
                            rel="noopener noreferrer"
                            class="text-[var(--color-primary)] truncate hover:underline"
                        >
                            {new URL(data.pkg.homepage_url).hostname}
                        </a>
                    </div>
                {/if}
                {#if data.pkg.repository_url}
                    <div class="flex justify-between gap-2 min-w-0">
                        <span class="text-[var(--color-text-muted)] shrink-0"
                            >Repository</span
                        >
                        <a
                            href={data.pkg.repository_url}
                            target="_blank"
                            rel="noopener noreferrer"
                            class="text-[var(--color-primary)] truncate hover:underline"
                        >
                            {new URL(data.pkg.repository_url).hostname}
                        </a>
                    </div>
                {/if}
                <div class="flex justify-between">
                    <span class="text-[var(--color-text-muted)]">Updated</span>
                    <span class="text-white"
                        >{timeAgo(data.pkg.updated_at)}</span
                    >
                </div>
            </div>

            {#if data.versions.length}
                <div
                    class="p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
                >
                    <h3 class="text-sm font-semibold text-white mb-3">
                        Versions
                    </h3>
                    <div class="space-y-1.5">
                        {#each data.versions.slice(0, 8) as v}
                            <a
                                href="/packages/{data.pkg
                                    .name}/versions/{v.version}"
                                class="flex items-center justify-between text-xs hover:text-[var(--color-primary)] transition-colors"
                                class:text-[var(--color-text-muted)]={v.yanked}
                                class:text-[var(--color-text)]={!v.yanked}
                            >
                                <span class="font-mono">{v.version}</span>
                                {#if v.yanked}
                                    <span class="text-[var(--color-warning)]"
                                        >yanked</span
                                    >
                                {:else}
                                    <span class="text-[var(--color-text-muted)]"
                                        >{timeAgo(v.published_at)}</span
                                    >
                                {/if}
                            </a>
                        {/each}
                    </div>
                </div>
            {/if}
        </aside>
    </div>
</div>

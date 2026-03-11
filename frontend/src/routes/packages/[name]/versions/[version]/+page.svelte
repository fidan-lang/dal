<script lang="ts">
	import { formatBytes, renderMarkdown, timeAgo } from '$lib/utils';
	import { onMount } from 'svelte';

	let { data } = $props();
	let readme = $state('');

	onMount(() => {
		if (data.version.readme) {
			readme = renderMarkdown(data.version.readme);
		}
	});
</script>

<svelte:head>
	<title>{data.pkgName} v{data.version.version} — Dal</title>
</svelte:head>

<div class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
	<nav class="text-sm text-[var(--color-text-muted)] mb-6">
		<a href="/packages" class="hover:text-[var(--color-text)]">Packages</a>
		<span class="mx-2">›</span>
		<a href="/packages/{data.pkgName}" class="hover:text-[var(--color-text)]">{data.pkgName}</a>
		<span class="mx-2">›</span>
		<span class="text-[var(--color-text)]">v{data.version.version}</span>
	</nav>

	<div class="flex items-center gap-4 mb-6">
		<h1 class="text-2xl font-bold text-white font-mono">{data.pkgName}</h1>
		<span class="font-mono text-[var(--color-primary)] bg-[var(--color-primary)]/10 border border-[var(--color-primary)]/30 px-3 py-1 rounded-full text-sm">
			v{data.version.version}
		</span>
		{#if data.version.yanked}
			<span class="text-xs text-[var(--color-warning)] bg-[var(--color-warning)]/10 border border-[var(--color-warning)]/30 px-2 py-1 rounded">yanked</span>
		{/if}
	</div>

	<div class="grid grid-cols-1 lg:grid-cols-[1fr_240px] gap-8">
		<!-- README -->
		<div>
			{#if readme}
				<div class="prose">{@html readme}</div>
			{:else}
				<p class="text-[var(--color-text-muted)] text-sm">No readme for this version.</p>
			{/if}
		</div>

		<!-- Meta -->
		<aside class="space-y-4">
			<div class="p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)] text-sm space-y-2.5">
				<div class="flex justify-between">
					<span class="text-[var(--color-text-muted)]">Published</span>
					<span class="text-white">{timeAgo(data.version.published_at)}</span>
				</div>
				<div class="flex justify-between">
					<span class="text-[var(--color-text-muted)]">Size</span>
					<span class="text-white">{formatBytes(data.version.size_bytes)}</span>
				</div>
				<div class="flex justify-between">
					<span class="text-[var(--color-text-muted)]">Downloads</span>
					<span class="text-white">{data.version.downloads.toLocaleString()}</span>
				</div>
			</div>
			<div class="p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]">
				<p class="text-xs text-[var(--color-text-muted)] mb-2">Checksum (sha256)</p>
				<code class="font-mono text-xs text-[var(--color-text)] break-all">{data.version.checksum}</code>
			</div>
		</aside>
	</div>
</div>

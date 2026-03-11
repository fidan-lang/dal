<script lang="ts">
	import PackageCard from '$lib/components/PackageCard.svelte';
	import { timeAgo } from '$lib/utils';

	let { data } = $props();
</script>

<svelte:head>
	<title>{data.user.display_name ?? data.user.username} — Dal</title>
</svelte:head>

<div class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
	<!-- Profile header -->
	<div class="flex items-start gap-5 mb-10">
		<div class="w-16 h-16 rounded-full bg-[var(--color-primary)] text-white text-2xl font-bold flex items-center justify-center uppercase shrink-0">
			{data.user.username[0]}
		</div>
		<div>
			<h1 class="text-2xl font-bold text-white">{data.user.display_name ?? data.user.username}</h1>
			<p class="text-[var(--color-text-muted)] text-sm font-mono">@{data.user.username}</p>
			{#if data.user.bio}
				<p class="text-[var(--color-text)] text-sm mt-2">{data.user.bio}</p>
			{/if}
			<div class="flex items-center gap-4 mt-3 text-xs text-[var(--color-text-muted)]">
				<span>Joined {timeAgo(data.user.created_at)}</span>
				{#if data.user.website_url}
					<a href={data.user.website_url} target="_blank" rel="noopener noreferrer" class="text-[var(--color-primary)] hover:underline">
						{new URL(data.user.website_url).hostname}
					</a>
				{/if}
			</div>
		</div>
	</div>

	<!-- Packages -->
	<h2 class="text-lg font-semibold text-white mb-4">
		Packages
		<span class="text-sm font-normal text-[var(--color-text-muted)]">({data.packages.total})</span>
	</h2>
	{#if data.packages.items.length}
		<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
			{#each data.packages.items as pkg}
				<PackageCard {pkg} />
			{/each}
		</div>
	{:else}
		<p class="text-[var(--color-text-muted)] text-sm">No packages published yet.</p>
	{/if}
</div>

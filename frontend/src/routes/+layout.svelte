<script lang="ts">
	import { onNavigate } from "$app/navigation";
	import { page } from "$app/state";
	import { env } from "$env/dynamic/public";
	import Footer from "$lib/components/Footer.svelte";
	import Nav from "$lib/components/Nav.svelte";
	import { authLoading, currentUser } from "$lib/stores/auth";
	import { onMount } from "svelte";
	import "../app.css";

	let { data, children } = $props();

	const CANONICAL_BASE_URL =
		env.PUBLIC_CANONICAL_BASE_URL ?? "https://dal.fidan.dev";
	let canonical = $derived(`${CANONICAL_BASE_URL}${page.url.pathname}`);

	onMount(() => {
		currentUser.set(data.user ?? null);
		authLoading.set(false);
	});

	onNavigate((navigation) => {
		if (!document.startViewTransition) return;
		return new Promise((resolve) => {
			document.startViewTransition(async () => {
				resolve();
				await navigation.complete;
			});
		});
	});
</script>

<svelte:head>
	<link rel="canonical" href={canonical} />
</svelte:head>

<div class="min-h-screen flex flex-col">
	<Nav />
	<main class="flex-1">
		{@render children()}
	</main>
	<Footer />
</div>

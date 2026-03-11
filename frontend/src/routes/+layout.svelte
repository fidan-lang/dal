<script lang="ts">
	import { onNavigate } from "$app/navigation";
	import Footer from "$lib/components/Footer.svelte";
	import Nav from "$lib/components/Nav.svelte";
	import { authLoading, currentUser } from "$lib/stores/auth";
	import { onMount } from "svelte";
	import "../app.css";

	let { data, children } = $props();

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

<div class="min-h-screen flex flex-col">
	<Nav />
	<main class="flex-1">
		{@render children()}
	</main>
	<Footer />
</div>

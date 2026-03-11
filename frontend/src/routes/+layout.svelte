<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { currentUser, authLoading } from '$lib/stores/auth';
	import Nav from '$lib/components/Nav.svelte';
	import Footer from '$lib/components/Footer.svelte';

	let { data, children } = $props();

	onMount(() => {
		currentUser.set(data.user ?? null);
		authLoading.set(false);
	});
</script>

<div class="min-h-screen flex flex-col">
	<Nav />
	<main class="flex-1">
		{@render children()}
	</main>
	<Footer />
</div>

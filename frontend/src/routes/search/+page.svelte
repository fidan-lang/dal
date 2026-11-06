<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import PackageCard from "$lib/components/PackageCard.svelte";
  import Pagination from "$lib/components/Pagination.svelte";
  import { formatNumber } from "$lib/utils";
  let { data } = $props();
  let q = $state("");
  let category = $state("");
  $effect(() => {
    q = data.q;
    category = data.category;
  });

  function search(e: Event) {
    e.preventDefault();
    const u = new URL(page.url);
    if (q.trim()) u.searchParams.set("q", q.trim());
    else u.searchParams.delete("q");
    if (category.trim()) u.searchParams.set("category", category.trim());
    else u.searchParams.delete("category");
    u.searchParams.set("page", "1");
    goto(u.toString());
  }

  function changePage(p: number) {
    const u = new URL(page.url);
    u.searchParams.set("page", String(p));
    goto(u.toString());
  }
</script>

<svelte:head>
  <title
    >{data.q || data.category
      ? `Search — ${data.q || data.category}`
      : "Search"} — Dal</title
  >
</svelte:head>

<div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
  <h1 class="text-2xl font-bold text-white mb-6">Search packages</h1>

  <form
    onsubmit={search}
    class="flex flex-col sm:flex-row gap-2 mb-8 max-w-3xl transition-all duration-300"
  >
    <input
      type="search"
      aria-label="Search packages"
      bind:value={q}
      placeholder="Search names, keywords, descriptions…"
      class="w-full min-[540px]:flex-1 min-w-0 px-4 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-all duration-300 text-sm"
    />

    <input
      type="search"
      aria-label="Filter by category"
      bind:value={category}
      placeholder="Category (optional)"
      class="w-full sm:w-48 min-w-0 px-4 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-all duration-300 text-sm"
    />

    <button
      type="submit"
      class="w-full min-[540px]:w-auto px-5 py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] text-white font-medium rounded-[var(--radius-md)] transition-all duration-300 text-sm"
    >
      Search
    </button>
  </form>

  {#if data.q || data.category}
    <p class="text-sm text-[var(--color-text-muted)] mb-5">
      {formatNumber(data.total)} result{data.total !== 1 ? "s" : ""}
      {#if data.q}
        for <strong class="text-[var(--color-text)]">"{data.q}"</strong>
      {/if}
      {#if data.category}
        in category <strong class="text-[var(--color-text)]"
          >{data.category}</strong
        >
      {/if}
    </p>
  {/if}

  {#if data.items.length}
    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 mb-10">
      {#each data.items as pkg}
        <PackageCard {pkg} />
      {/each}
    </div>
    <div class="flex justify-center">
      <Pagination
        total={data.total}
        page={data.page}
        pages={data.pages}
        onchange={changePage}
      />
    </div>
  {:else if data.q || data.category}
    <div class="text-center py-16 text-[var(--color-text-muted)]">
      <p>No packages found. Try a different query.</p>
    </div>
  {/if}
</div>

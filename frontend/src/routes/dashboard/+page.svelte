<script lang="ts">
  import { DalApiError, owners as ownersApi } from "$lib/api";
  import PackageCard from "$lib/components/PackageCard.svelte";
  import { currentUser } from "$lib/stores/auth";
  import { formatNumber } from "$lib/utils";

  let { data } = $props();
  let totalDownloads = $derived(
    data.packages.items.reduce((a, p) => a + p.downloads, 0),
  );
  let pendingInvites = $state<typeof data.pendingInvites>([]);
  let inviteError = $state("");
  let inviteBusy = $state<string | null>(null);

  $effect(() => {
    pendingInvites = data.pendingInvites;
  });

  async function respondToInvite(id: string, accept: boolean) {
    inviteBusy = `${accept ? "accept" : "decline"}:${id}`;
    inviteError = "";

    try {
      if (accept) {
        await ownersApi.acceptInvite(fetch, id);
      } else {
        await ownersApi.declineInvite(fetch, id);
      }

      pendingInvites = pendingInvites.filter((invite) => invite.id !== id);
    } catch (err) {
      inviteError =
        err instanceof DalApiError ? err.message : "Failed to update invite.";
    } finally {
      inviteBusy = null;
    }
  }
</script>

<svelte:head>
  <title>Dashboard — Dal</title>
</svelte:head>

<div class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
  <div class="flex items-start justify-between mb-8">
    <div>
      <h1 class="text-2xl font-bold text-white">Dashboard</h1>
      {#if $currentUser}
        <p class="text-[var(--color-text-muted)] text-sm mt-1">
          Welcome back, <span class="text-[var(--color-text)]"
            >{$currentUser.display_name ?? $currentUser.username}</span
          >
          {#if $currentUser.is_admin}
            <span
              class="ml-2 text-xs text-[var(--color-primary-light)] bg-[var(--color-primary)]/10 border border-[var(--color-primary)]/30 px-2 py-0.5 rounded"
              >platform admin</span
            >
          {/if}
          {#if !$currentUser.email_verified}
            <span
              class="ml-2 text-xs text-[var(--color-warning)] bg-[var(--color-warning)]/10 border border-[var(--color-warning)]/30 px-2 py-0.5 rounded"
              >email unverified</span
            >
          {/if}
        </p>
      {/if}
    </div>
  </div>

  {#if $currentUser?.is_admin}
    <div
      class="mb-8 px-4 py-3 bg-[var(--color-primary)]/8 border border-[var(--color-primary)]/20 rounded-[var(--radius-md)] text-sm text-[var(--color-text)]"
    >
      Your account has platform admin powers. You can moderate any package from
      its package page or management screen, even if you are not an owner.
    </div>
  {/if}

  {#if pendingInvites.length || inviteError}
    <section class="mb-10">
      <div class="flex items-center justify-between gap-4 mb-4">
        <div>
          <h2 class="text-lg font-semibold text-white">Pending invites</h2>
          <p class="text-sm text-[var(--color-text-muted)] mt-1">
            Review package collaboration invites before they expire.
          </p>
        </div>
        {#if pendingInvites.length}
          <span
            class="px-3 py-1 rounded-full bg-[var(--color-primary)]/10 text-[var(--color-primary-light)] text-xs font-medium border border-[var(--color-primary)]/20"
          >
            {pendingInvites.length} pending
          </span>
        {/if}
      </div>

      {#if inviteError}
        <div
          class="mb-4 px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
        >
          {inviteError}
        </div>
      {/if}

      {#if pendingInvites.length}
        <div class="space-y-3">
          {#each pendingInvites as invite}
            <div
              class="p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)] flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between"
            >
              <div>
                <p class="text-sm font-medium text-white">
                  {invite.package_name}
                </p>
                <p class="mt-1 text-sm text-[var(--color-text-muted)]">
                  {invite.inviter_display_name ?? invite.inviter_username}
                  invited you to join as {invite.role}.
                </p>
                <p class="mt-2 text-xs text-[var(--color-text-muted)]">
                  Sent {new Date(invite.created_at).toLocaleDateString()} · expires
                  {new Date(invite.expires_at).toLocaleDateString()}
                </p>
              </div>
              <div class="flex gap-2">
                <button
                  type="button"
                  disabled={inviteBusy !== null}
                  onclick={() => respondToInvite(invite.id, false)}
                  class="px-4 py-2 border border-[var(--color-border)] rounded-[var(--radius-md)] text-sm text-[var(--color-text)] hover:border-[var(--color-text-muted)] disabled:opacity-60 transition-colors"
                >
                  Decline
                </button>
                <button
                  type="button"
                  disabled={inviteBusy !== null}
                  onclick={() => respondToInvite(invite.id, true)}
                  class="px-4 py-2 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white text-sm font-medium rounded-[var(--radius-md)] transition-colors"
                >
                  {inviteBusy === `accept:${invite.id}`
                    ? "Accepting..."
                    : "Accept"}
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  <!-- Summary cards -->
  <div class="grid grid-cols-1 sm:grid-cols-3 gap-4 mb-10">
    <div
      class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
    >
      <div class="text-2xl font-bold text-white">
        {data.packages.total}
      </div>
      <div class="text-sm text-[var(--color-text-muted)] mt-1">Packages</div>
    </div>
    <div
      class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
    >
      <div class="text-2xl font-bold text-white">
        {formatNumber(totalDownloads)}
      </div>
      <div class="text-sm text-[var(--color-text-muted)] mt-1">
        Total downloads
      </div>
    </div>
    <div
      class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
    >
      <a href="/settings/tokens" class="block">
        <div class="text-2xl font-bold text-[var(--color-primary)]">→</div>
        <div class="text-sm text-[var(--color-text-muted)] mt-1">
          API Tokens
        </div>
      </a>
    </div>
  </div>

  <!-- Package list -->
  <h2 class="text-lg font-semibold text-white mb-4">Your packages</h2>
  {#if data.packages.items.length}
    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      {#each data.packages.items as pkg}
        <PackageCard {pkg} />
      {/each}
    </div>
  {:else}
    <div class="text-center py-16 text-[var(--color-text-muted)]">
      <p>You haven't published any packages yet.</p>
      <p class="text-sm mt-2">
        Run <code class="font-mono text-[var(--color-primary-light)]"
          >fidan dal publish</code
        > in your project to get started.
      </p>
    </div>
  {/if}
</div>

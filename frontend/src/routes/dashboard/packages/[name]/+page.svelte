<script lang="ts">
  import { goto } from "$app/navigation";
  import {
    admin as adminApi,
    DalApiError,
    owners as ownersApi,
    versions as versionsApi,
  } from "$lib/api";
  import { formatBytes, timeAgo } from "$lib/utils";

  let { data } = $props();

  let ownersList = $state<typeof data.owners>([]);
  let versionList = $state<typeof data.versions>([]);
  let inviteUsername = $state("");
  let inviteRole = $state<"owner" | "collaborator">("collaborator");
  let transferUsername = $state("");
  let error = $state("");
  let success = $state("");
  let busyAction = $state<string | null>(null);
  let canManageOwners = $derived(
    data.currentRole === "owner" || data.isAdmin,
  );

  $effect(() => {
    ownersList = data.owners;
    versionList = data.versions;
  });

  function begin(action: string) {
    busyAction = action;
    error = "";
    success = "";
  }

  function finish(message?: string) {
    busyAction = null;
    if (message) {
      success = message;
    }
  }

  function fail(err: unknown, fallback: string) {
    busyAction = null;
    error = err instanceof DalApiError ? err.message : fallback;
  }

  async function inviteOwner(event: Event) {
    event.preventDefault();
    begin("invite");

    try {
      await ownersApi.invite(fetch, data.pkg.name, inviteUsername, inviteRole);
      inviteUsername = "";
      inviteRole = "collaborator";
      finish("Invite sent.");
    } catch (err) {
      fail(err, "Failed to send invite.");
    }
  }

  async function removeOwner(username: string) {
    begin(`remove:${username}`);

    try {
      await ownersApi.remove(fetch, data.pkg.name, username);
      ownersList = ownersList.filter((owner) => owner.username !== username);
      finish("Owner removed.");
    } catch (err) {
      fail(err, "Failed to remove owner.");
    }
  }

  async function toggleYank(version: string, yanked: boolean) {
    begin(`${yanked ? "unyank" : "yank"}:${version}`);

    try {
      if (yanked) {
        await versionsApi.unyank(fetch, data.pkg.name, version);
      } else {
        await versionsApi.yank(fetch, data.pkg.name, version);
      }

      versionList = versionList.map((entry) =>
        entry.version === version ? { ...entry, yanked: !yanked } : entry,
      );
      finish(yanked ? "Version restored." : "Version yanked.");
    } catch (err) {
      fail(
        err,
        yanked ? "Failed to restore version." : "Failed to yank version.",
      );
    }
  }

  async function transferOwnership(event: Event) {
    event.preventDefault();
    begin("transfer");

    try {
      await ownersApi.transfer(fetch, data.pkg.name, transferUsername);
      ownersList = await ownersApi.list(fetch, data.pkg.name);
      finish("Ownership transferred.");
      transferUsername = "";
      await goto(`/packages/${data.pkg.name}`);
    } catch (err) {
      fail(err, "Failed to transfer ownership.");
    }
  }

  async function deletePackage() {
    if (
      !window.confirm(
        `Delete ${data.pkg.name} from Dal permanently? This removes all versions and stored archives.`,
      )
    ) {
      return;
    }

    begin("delete-package");

    try {
      await adminApi.deletePackage(fetch, data.pkg.name);
      await goto("/packages");
    } catch (err) {
      fail(err, "Failed to delete package.");
    }
  }
</script>

<svelte:head>
  <title>Manage {data.pkg.name} — Dal</title>
</svelte:head>

<div class="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8 py-10 space-y-10">
  <div class="flex items-start justify-between gap-4 flex-wrap">
    <div>
      <a
        href="/packages/{data.pkg.name}"
        class="text-sm text-[var(--color-primary)] hover:underline"
      >
        ← Back to package
      </a>
      <h1 class="mt-3 text-3xl font-bold text-white font-mono">
        Manage {data.pkg.name}
      </h1>
      <div class="mt-3 flex flex-wrap items-center gap-2">
        <span
          class="text-xs text-[var(--color-text-muted)] bg-[var(--color-surface-2)] border border-[var(--color-border)] px-2 py-0.5 rounded"
        >
          access: {data.currentRole}
        </span>
        {#if data.isAdmin}
          <span
            class="text-xs text-[var(--color-primary-light)] bg-[var(--color-primary)]/10 border border-[var(--color-primary)]/30 px-2 py-0.5 rounded"
          >
            platform admin
          </span>
        {/if}
      </div>
      {#if data.pkg.description}
        <p class="mt-2 text-[var(--color-text-muted)]">
          {data.pkg.description}
        </p>
      {/if}
    </div>
  </div>

  {#if error}
    <div
      class="px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
    >
      {error}
    </div>
  {/if}

  {#if success}
    <div
      class="px-4 py-3 bg-[var(--color-success)]/10 border border-[var(--color-success)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-success)]"
    >
      {success}
    </div>
  {/if}

  <section class="space-y-4">
    <div>
      <h2 class="text-lg font-semibold text-white">Versions</h2>
      <p class="text-sm text-[var(--color-text-muted)] mt-1">
        Yank or restore published versions.
      </p>
    </div>

    <div class="space-y-3">
      {#each versionList as version}
        <div
          class="flex items-center justify-between gap-4 p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
        >
          <div>
            <a
              href="/packages/{data.pkg.name}/versions/{version.version}"
              class="font-mono text-[var(--color-primary)] hover:underline"
            >
              v{version.version}
            </a>
            <div class="mt-1 text-xs text-[var(--color-text-muted)] flex gap-3">
              <span>{formatBytes(version.size_bytes)}</span>
              <span>{timeAgo(version.published_at)}</span>
            </div>
          </div>
          <button
            type="button"
            disabled={busyAction === `yank:${version.version}` ||
              busyAction === `unyank:${version.version}`}
            onclick={() => toggleYank(version.version, version.yanked)}
            class={`px-3 py-2 rounded-[var(--radius-sm)] text-sm font-medium transition-colors ${
              version.yanked
                ? "bg-[var(--color-primary)]/15 text-[var(--color-primary-light)] hover:bg-[var(--color-primary)]/25"
                : "bg-[var(--color-danger)]/15 text-[var(--color-danger)] hover:bg-[var(--color-danger)]/25"
            }`}
          >
            {version.yanked ? "Unyank" : "Yank"}
          </button>
        </div>
      {/each}
    </div>
  </section>

  <section class="grid gap-8 lg:grid-cols-[1.2fr_0.8fr]">
    <div class="space-y-4">
      <div>
        <h2 class="text-lg font-semibold text-white">Members</h2>
        <p class="text-sm text-[var(--color-text-muted)] mt-1">
          Package members can publish and manage versions.
        </p>
      </div>

      <div class="space-y-3">
        {#each ownersList as owner}
          <div
            class="flex items-center justify-between gap-4 p-4 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-md)]"
          >
            <div>
              <p class="text-sm text-white">
                {owner.display_name ?? owner.username}
                {#if owner.username === data.currentUsername}
                  <span class="text-[var(--color-text-muted)]"> (you) </span>
                {/if}
              </p>
              <p class="text-xs text-[var(--color-text-muted)] mt-1">
                @{owner.username} · {owner.role} · added {timeAgo(
                  owner.added_at,
                )}
              </p>
            </div>
            {#if canManageOwners && (ownersList.length > 1 || data.isAdmin)}
              <button
                type="button"
                disabled={busyAction === `remove:${owner.username}`}
                onclick={() => removeOwner(owner.username)}
                class="text-sm text-[var(--color-danger)] hover:underline disabled:opacity-60"
              >
                Remove
              </button>
            {/if}
          </div>
        {/each}
      </div>
    </div>

    <div class="space-y-8">
      {#if canManageOwners}
        <section
          class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
          <h3 class="text-base font-semibold text-white">Add member</h3>
          <p class="mt-2 text-sm text-[var(--color-text-muted)]">
            Owners can invite collaborators or another owner. Invitations stay
            pending until accepted.
          </p>
          <form onsubmit={inviteOwner} class="mt-4 space-y-3">
            <label
              for="invite-username"
              class="block text-sm font-medium text-[var(--color-text)]"
            >
              Username
            </label>
            <input
              id="invite-username"
              type="text"
              bind:value={inviteUsername}
              required
              placeholder="Username"
              class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none text-sm"
            />
            <label
              for="invite-role"
              class="block text-sm font-medium text-[var(--color-text)]"
            >
              Role
            </label>
            <select
              id="invite-role"
              bind:value={inviteRole}
              class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] focus:border-[var(--color-primary)] focus:outline-none text-sm"
            >
              <option value="collaborator">Collaborator</option>
              <option value="owner">Owner</option>
            </select>
            <button
              type="submit"
              disabled={busyAction === "invite"}
              class="w-full py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white text-sm font-medium rounded-[var(--radius-md)] transition-colors"
            >
              {busyAction === "invite" ? "Sending..." : "Send invite"}
            </button>
          </form>
        </section>

        <section
          class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
          <h3 class="text-base font-semibold text-white">Transfer ownership</h3>
          <p class="mt-2 text-sm text-[var(--color-text-muted)]">
            Promote another user and demote yourself to collaborator.
          </p>
          <form onsubmit={transferOwnership} class="mt-4 space-y-3">
            <label
              for="transfer-username"
              class="block text-sm font-medium text-[var(--color-text)]"
            >
              Recipient username
            </label>
            <input
              id="transfer-username"
              type="text"
              bind:value={transferUsername}
              required
              placeholder="Recipient username"
              class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none text-sm"
            />
            <button
              type="submit"
              disabled={busyAction === "transfer"}
              class="w-full py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white text-sm font-medium rounded-[var(--radius-md)] transition-colors"
            >
              {busyAction === "transfer"
                ? "Transferring…"
                : "Transfer ownership"}
            </button>
          </form>
        </section>
      {:else}
        <section
          class="p-5 bg-[var(--color-surface-2)] border border-[var(--color-border)] rounded-[var(--radius-lg)]"
        >
          <h3 class="text-base font-semibold text-white">
            Collaborator access
          </h3>
          <p class="mt-2 text-sm text-[var(--color-text-muted)]">
            You can publish and manage versions, but only owners can change
            package membership.
          </p>
        </section>
      {/if}

      {#if data.isAdmin}
        <section
          class="p-5 bg-[var(--color-danger)]/8 border border-[var(--color-danger)]/30 rounded-[var(--radius-lg)]"
        >
          <h3 class="text-base font-semibold text-white">
            Platform moderation
          </h3>
          <p class="mt-2 text-sm text-[var(--color-text-muted)]">
            As a platform admin, you can permanently remove this package from
            the registry if it is malicious or violates policy.
          </p>
          <button
            type="button"
            disabled={busyAction === "delete-package"}
            onclick={deletePackage}
            class="mt-4 w-full py-2.5 bg-[var(--color-danger)] hover:bg-[var(--color-danger)]/80 disabled:opacity-60 text-white text-sm font-medium rounded-[var(--radius-md)] transition-colors"
          >
            {busyAction === "delete-package"
              ? "Deleting..."
              : "Delete package permanently"}
          </button>
        </section>
      {/if}
    </div>
  </section>
</div>

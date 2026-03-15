<script lang="ts">
    import { DalApiError, users } from "$lib/api";
    import { currentUser } from "$lib/stores/auth";

    let displayName = $state($currentUser?.display_name ?? "");
    let bio = $state($currentUser?.bio ?? "");
    let websiteUrl = $state($currentUser?.website_url ?? "");
    let lastHydratedProfile = $state<string | null>(null);
    let success = $state(false);
    let error = $state("");
    let loading = $state(false);

    $effect(() => {
        if (!$currentUser) return;

        const profileSignature = JSON.stringify([
            $currentUser.id,
            $currentUser.display_name ?? "",
            $currentUser.bio ?? "",
            $currentUser.website_url ?? "",
        ]);

        if (profileSignature === lastHydratedProfile) return;

        displayName = $currentUser.display_name ?? "";
        bio = $currentUser.bio ?? "";
        websiteUrl = $currentUser.website_url ?? "";
        lastHydratedProfile = profileSignature;
    });

    async function handleSubmit(e: Event) {
        e.preventDefault();
        error = "";
        success = false;
        loading = true;
        try {
            const nextDisplayName = displayName.trim();
            const nextBio = bio.trim();
            const nextWebsiteUrl = websiteUrl.trim();
            const updated = await users.updateProfile(fetch, {
                display_name: nextDisplayName,
                bio: nextBio,
                website_url: nextWebsiteUrl,
            });
            currentUser.set(updated);
            success = true;
        } catch (err) {
            error =
                err instanceof DalApiError
                    ? err.message
                    : "Failed to save profile.";
        } finally {
            loading = false;
        }
    }
</script>

<svelte:head>
    <title>Profile settings — Dal</title>
</svelte:head>

<div>
    <h2 class="text-lg font-semibold text-white mb-6">Profile</h2>
    <form onsubmit={handleSubmit} class="space-y-5 max-w-lg">
        {#if success}
            <div
                class="px-4 py-3 bg-[var(--color-success)]/10 border border-[var(--color-success)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-success)]"
            >
                Profile saved.
            </div>
        {/if}
        {#if error}
            <div
                class="px-4 py-3 bg-[var(--color-danger)]/10 border border-[var(--color-danger)]/30 rounded-[var(--radius-md)] text-sm text-[var(--color-danger)]"
            >
                {error}
            </div>
        {/if}

        <div>
            <label
                for="username-display"
                class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                >Username</label
            >
            <input
                id="username-display"
                type="text"
                value={$currentUser?.username ?? ""}
                disabled
                class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text-muted)] text-sm opacity-60 cursor-not-allowed"
            />
            <p class="text-xs text-[var(--color-text-muted)] mt-1">
                Username cannot be changed.
            </p>
        </div>

        <div>
            <label
                for="display_name"
                class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                >Display name</label
            >
            <input
                id="display_name"
                type="text"
                bind:value={displayName}
                maxlength="64"
                class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
                placeholder="Your Name"
            />
        </div>

        <div>
            <label
                for="bio"
                class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                >Bio</label
            >
            <textarea
                id="bio"
                bind:value={bio}
                maxlength="256"
                rows="3"
                class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm resize-none"
                placeholder="Tell the world about yourself"
            ></textarea>
        </div>

        <div>
            <label
                for="website"
                class="block text-sm font-medium text-[var(--color-text)] mb-1.5"
                >Website URL</label
            >
            <input
                id="website"
                type="url"
                bind:value={websiteUrl}
                class="w-full px-3 py-2.5 bg-[var(--color-surface-3)] border border-[var(--color-border)] rounded-[var(--radius-md)] text-[var(--color-text)] placeholder-[var(--color-text-muted)] focus:border-[var(--color-primary)] focus:outline-none transition-colors text-sm"
                placeholder="https://yoursite.com"
            />
        </div>

        <button
            type="submit"
            disabled={loading}
            class="px-5 py-2.5 bg-[var(--color-primary)] hover:bg-[var(--color-primary-dark)] disabled:opacity-60 text-white font-medium rounded-[var(--radius-md)] transition-colors text-sm"
        >
            {loading ? "Saving…" : "Save profile"}
        </button>
    </form>
</div>

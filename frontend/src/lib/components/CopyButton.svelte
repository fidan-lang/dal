<script lang="ts">
  import { onDestroy } from "svelte";

  let { text, label }: { text: string; label: string } = $props();
  let copied = $state(false);
  let failed = $state(false);
  let lastCopiedText = $state("");
  let resetTimer: ReturnType<typeof setTimeout> | undefined;
  const copySucceeded = $derived(copied && lastCopiedText === text);

  onDestroy(() => {
    if (resetTimer) clearTimeout(resetTimer);
  });

  async function copyText() {
    if (!text) return;
    if (resetTimer) clearTimeout(resetTimer);
    copied = false;
    failed = false;
    const value = text;
    let success = false;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(value);
        success = true;
      }
    } catch {
      // Clipboard permission may be denied; attempt the legacy fallback.
    }
    if (!success) {
      const field = document.createElement("textarea");
      field.value = value;
      field.readOnly = true;
      field.style.position = "fixed";
      field.style.left = "-9999px";
      document.body.appendChild(field);
      try {
        field.select();
        success = document.execCommand("copy");
      } catch {
        success = false;
      } finally {
        field.remove();
      }
    }
    if (value !== text) return;
    lastCopiedText = value;
    copied = success;
    failed = !success;
    resetTimer = setTimeout(() => {
      copied = false;
      failed = false;
    }, 2000);
  }
</script>

<button
  type="button"
  onclick={copyText}
  disabled={!text}
  aria-label={copySucceeded ? `${label} copied` : `Copy ${label}`}
  title={failed ? "Copy failed" : copySucceeded ? "Copied!" : `Copy ${label}`}
  class="flex h-9 w-9 shrink-0 items-center justify-center rounded-[var(--radius-sm)] border border-[var(--color-border)] text-[var(--color-text-muted)] transition-colors hover:border-[var(--color-primary)]/50 hover:bg-[var(--color-primary)]/10 hover:text-[var(--color-primary-light)] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-primary)] disabled:opacity-50"
>
  <svg
    aria-hidden="true"
    class="h-[18px] w-[18px]"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    {#if copySucceeded}
      <path d="M20 6 9 17l-5-5" />
    {:else}
      <rect width="14" height="14" x="8" y="8" rx="2" />
      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
    {/if}
  </svg>
</button>
<span class="sr-only" aria-live="polite">
  {failed
    ? `Could not copy ${label}. Please select and copy it manually.`
    : copySucceeded
      ? `${label} copied to clipboard.`
      : ""}
</span>

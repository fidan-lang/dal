<script lang="ts">
    import { onMount, onDestroy } from "svelte";
    import { goto } from "$app/navigation";
    import QRCode from "qrcode";
    import { mfa } from "$lib/api";
    import { currentUser } from "$lib/stores/auth";
    let enabled = $state<boolean | null>(null);
    let available = $state(false);
    let password = $state("");
    let currentCode = $state("");
    let code = $state("");
    let secret = $state("");
    let uri = $state("");
    let qr = $state("");
    let error = $state("");
    let busy = $state(true);
    let generation = 0;
    let expiry: ReturnType<typeof setTimeout> | undefined;
    function clearSetup() {
        generation++;
        secret = "";
        uri = "";
        qr = "";
        code = "";
        if (expiry) clearTimeout(expiry);
    }
    onDestroy(clearSetup);
    onMount(async () => {
        try {
            const status = await mfa.status(fetch);
            enabled = status.enabled;
            available = status.available;
        } catch {
            error =
                "Unable to load authenticator status. Try again before making changes.";
        } finally {
            busy = false;
        }
    });
    async function setup() {
        busy = true;
        error = "";
        clearSetup();
        const epoch = generation;
        try {
            const result = await mfa.setup(
                fetch,
                password,
                enabled ? currentCode : undefined,
            );
            if (epoch !== generation) return;
            password = "";
            currentCode = "";
            secret = result.secret;
            uri = result.uri;
            const generated = await QRCode.toDataURL(uri, {
                width: 240,
                margin: 2,
            });
            if (epoch !== generation) return;
            qr = generated;
            expiry = setTimeout(() => {
                clearSetup();
                error =
                    "Setup expired. Authenticate again to start a new setup.";
            }, result.expires_in * 1000);
        } catch {
            error =
                "Setup failed. Check your password and, if enabled, a fresh authenticator code. Restart setup after five failed verification attempts.";
        } finally {
            busy = false;
            password = "";
            currentCode = "";
        }
    }
    async function finish(disable = false) {
        busy = true;
        error = "";
        try {
            if (disable) await mfa.disable(fetch, password, currentCode);
            else await mfa.verify(fetch, code);
            clearSetup();
            password = "";
            currentCode = "";
            currentUser.set(null);
            await goto("/login");
        } catch {
            error =
                "Change failed. Check your credentials and use a fresh six-digit code. If setup expired or five attempts failed, start again.";
        } finally {
            busy = false;
            password = "";
            currentCode = "";
            code = "";
        }
    }
</script>

<svelte:head
    ><title>Account security — Dal</title><meta
        name="robots"
        content="noindex,nofollow"
    /></svelte:head
>
<div class="max-w-lg space-y-5">
    <h2 class="text-lg font-semibold text-white">Authenticator security</h2>
    <p>
        Authenticator MFA is {enabled === null
            ? "unknown until status loads"
            : enabled
              ? "enabled"
              : "not enabled"}. MFA is optional. Downloads never require an
        account.
    </p>
    {#if !available}<p>
            New authenticator enrollment is not yet available on this registry.
        </p>{/if}
    {#if error}<p role="alert" class="text-[var(--color-danger)]">
            {error}
        </p>{/if}
    {#if secret}
        <p>
            Scan this QR code in your authenticator, or enter the secret
            manually. Keep an offline backup in a safe place. This secret is
            shown only during setup.
        </p>
        {#if qr}<img
                src={qr}
                width="240"
                height="240"
                alt="Scan to add your DAL authenticator"
            />{/if}
        <p>
            Manual setup secret: <code class="break-all select-all"
                >{secret}</code
            >
        </p>
        <p>Account: DAL. Six digits, 30-second interval, SHA-1.</p>
        <form
            onsubmit={(event) => {
                event.preventDefault();
                void finish();
            }}
            class="space-y-4"
        >
            <label for="new-code">Code from the new authenticator</label>
            <input
                id="new-code"
                type="text"
                bind:value={code}
                pattern={"[0-9]{6}"}
                inputmode="numeric"
                autocomplete="one-time-code"
                maxlength="6"
                required
                class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
            />
            <button
                disabled={busy}
                class="px-5 py-2 bg-[var(--color-primary)] rounded-[var(--radius-md)]"
                >Verify and enable</button
            >
        </form>
        <button disabled={busy} onclick={clearSetup}>Hide setup secret</button>
    {:else}
        <form
            onsubmit={(event) => {
                event.preventDefault();
                void setup();
            }}
            class="space-y-4"
        >
            <label for="security-password">Current password</label>
            <input
                id="security-password"
                type="password"
                bind:value={password}
                autocomplete="current-password"
                required
                class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
            />
            {#if enabled}
                <label for="current-code">Current authenticator code</label>
                <input
                    id="current-code"
                    type="text"
                    bind:value={currentCode}
                    pattern={"[0-9]{6}"}
                    inputmode="numeric"
                    autocomplete="one-time-code"
                    maxlength="6"
                    required
                    class="w-full p-3 bg-[var(--color-surface-3)] rounded-[var(--radius-md)]"
                />
            {/if}
            <button
                disabled={busy || !available}
                class="px-5 py-2 bg-[var(--color-primary)] rounded-[var(--radius-md)]"
                >{enabled
                    ? "Replace authenticator"
                    : "Set up authenticator"}</button
            >
            {#if enabled}<button
                    type="button"
                    disabled={busy ||
                        !password ||
                        !/^[0-9]{6}$/.test(currentCode)}
                    onclick={() => {
                        void finish(true);
                    }}
                    class="px-5 py-2 text-[var(--color-danger)]"
                    >Disable authenticator</button
                >{/if}
        </form>
    {/if}
    <p>
        Changes require fresh password authentication and, when MFA is enabled,
        your current authenticator code. Successful changes sign out all browser
        sessions. Use the next code to sign in again.
    </p>
    <p>
        Lost authenticator? Restore your securely stored authenticator backup.
        Otherwise contact the registry operator for independent identity
        verification. Email access or a password reset alone cannot disable MFA;
        recovery can be refused when identity cannot be established.
    </p>
</div>

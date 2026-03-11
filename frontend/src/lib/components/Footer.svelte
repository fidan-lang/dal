<footer
    class="border-t border-[var(--color-border)] bg-[var(--color-surface-2)] mt-16"
>
    <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-10">
        <div class="grid grid-cols-2 md:grid-cols-4 gap-8">
            <div>
                <div class="flex items-center gap-2 font-bold text-white mb-4">
                    <svg
                        width="24"
                        height="24"
                        viewBox="0 0 28 28"
                        fill="none"
                        aria-hidden="true"
                    >
                        <rect width="28" height="28" rx="7" fill="#84cc16" />
                        <text
                            x="7"
                            y="20"
                            font-family="monospace"
                            font-size="14"
                            font-weight="700"
                            fill="white">d</text
                        >
                    </svg>
                    dal
                </div>
                <p class="text-sm text-[var(--color-text-muted)]">
                    The official package registry for the Fidan programming
                    language.
                </p>
            </div>
            <div>
                <h4 class="text-sm font-semibold text-white mb-3">Registry</h4>
                <ul class="space-y-2 text-sm text-[var(--color-text-muted)]">
                    <li>
                        <a
                            href="/packages"
                            class="hover:text-[var(--color-text)]"
                            >Browse packages</a
                        >
                    </li>
                    <li>
                        <a href="/search" class="hover:text-[var(--color-text)]"
                            >Search</a
                        >
                    </li>
                </ul>
            </div>
            <div>
                <h4 class="text-sm font-semibold text-white mb-3">Account</h4>
                <ul class="space-y-2 text-sm text-[var(--color-text-muted)]">
                    <li>
                        <a
                            href="/register"
                            class="hover:text-[var(--color-text)]">Sign up</a
                        >
                    </li>
                    <li>
                        <a href="/login" class="hover:text-[var(--color-text)]"
                            >Sign in</a
                        >
                    </li>
                    <li>
                        <a
                            href="/settings/tokens"
                            class="hover:text-[var(--color-text)]">API Tokens</a
                        >
                    </li>
                </ul>
            </div>
            <div>
                <h4 class="text-sm font-semibold text-white mb-3">Links</h4>
                <ul class="space-y-2 text-sm text-[var(--color-text-muted)]">
                    <li>
                        <a
                            href="https://github.com/fidan-lang/fidan"
                            target="_blank"
                            rel="noopener noreferrer"
                            class="hover:text-[var(--color-text)]"
                        >
                            GitHub
                        </a>
                    </li>
                    <li>
                        <a
                            href="https://fidan.dev"
                            target="_blank"
                            rel="noopener noreferrer"
                            class="hover:text-[var(--color-text)]"
                        >
                            Fidan Language
                        </a>
                    </li>
                </ul>
            </div>
        </div>
        <div
            class="border-t border-[var(--color-border)] mt-8 pt-6 text-center text-xs text-[var(--color-text-muted)]"
        >
            &copy; {new Date().getFullYear()} Dal / Fidan. All rights reserved.
        </div>
    </div>
</footer>

/**
 * Typed API client for the Dal registry backend.
 * All requests go through SvelteKit's fetch so SSR and CSR both work.
 */
import { env } from '$env/dynamic/public';

export const API_BASE = env.PUBLIC_API_URL ?? 'https://api.dal.fidan.dev';

// ── Types ────────────────────────────────────────────────────────────────────

export interface User {
    id: string;
    username: string;
    email: string;
    display_name: string | null;
    bio: string | null;
    website_url: string | null;
    email_verified: boolean;
    created_at: string;
}

export interface Package {
    id: string;
    name: string;
    description: string | null;
    homepage_url: string | null;
    repository_url: string | null;
    license: string | null;
    keywords: string[];
    downloads: number;
    latest_version: string | null;
    updated_at: string;
    created_at: string;
}

export interface PackageVersion {
    id: string;
    package_id: string;
    version: string;
    checksum: string;
    size_bytes: number;
    yanked: boolean;
    readme: string | null;
    manifest: Record<string, unknown>;
    downloads: number;
    published_at: string;
}

export interface PackageOwner {
    user_id: string;
    username: string;
    display_name: string | null;
    role: 'owner' | 'collaborator';
    added_at: string;
}

export interface ApiToken {
    id: string;
    name: string;
    prefix: string;
    last_used_at: string | null;
    created_at: string;
}

export interface Page<T> {
    items: T[];
    total: number;
    page: number;
    per_page: number;
    pages: number;
}

export interface ApiError {
    code: string;
    message: string;
}

// ── Client ───────────────────────────────────────────────────────────────────

type FetchFn = typeof fetch;

async function request<T>(
    fetchFn: FetchFn,
    method: string,
    path: string,
    body?: unknown
): Promise<T> {
    const res = await fetchFn(`${API_BASE}${path}`, {
        method,
        credentials: 'include',
        headers: body ? { 'Content-Type': 'application/json' } : undefined,
        body: body ? JSON.stringify(body) : undefined
    });

    if (!res.ok) {
        let errorMessage = `${res.status} ${res.statusText}`;
        try {
            const json: unknown = await res.json();
            if (
                typeof json === "object" &&
                json !== null &&
                "error" in json &&
                typeof json.error === "object" &&
                json.error !== null &&
                "message" in json.error &&
                typeof json.error.message === "string"
            ) {
                errorMessage = json.error.message;
            }
        } catch {
            // ignore
        }
        throw new DalApiError(res.status, errorMessage);
    }

    if (res.status === 204) return undefined as T;
    return res.json() as Promise<T>;
}

export class DalApiError extends Error {
    constructor(
        public status: number,
        message: string
    ) {
        super(message);
        this.name = 'DalApiError';
    }
}

// ── Auth ─────────────────────────────────────────────────────────────────────

export const auth = {
    register: (f: FetchFn, username: string, email: string, password: string) =>
        request<{ message: string }>(f, 'POST', '/auth/register', { username, email, password }),

    login: (f: FetchFn, username: string, password: string) =>
        request<User>(f, 'POST', '/auth/login', { username, password }),

    logout: (f: FetchFn) => request<void>(f, 'POST', '/auth/logout'),

    refresh: (f: FetchFn) => request<void>(f, 'POST', '/auth/refresh'),

    me: (f: FetchFn) => request<User>(f, 'GET', '/auth/me'),

    verifyEmail: (f: FetchFn, token: string) =>
        request<{ message: string }>(f, 'GET', `/auth/verify-email?token=${encodeURIComponent(token)}`),

    forgotPassword: (f: FetchFn, email: string) =>
        request<{ message: string }>(f, 'POST', '/auth/forgot-password', { email }),

    resetPassword: (f: FetchFn, token: string, new_password: string) =>
        request<{ message: string }>(f, 'POST', '/auth/reset-password', { token, new_password })
};

// ── Users ────────────────────────────────────────────────────────────────────

export const users = {
    get: (f: FetchFn, username: string) => request<User>(f, 'GET', `/users/${username}`),

    packages: (f: FetchFn, username: string, page = 1, per_page = 20) =>
        request<Page<Package>>(f, 'GET', `/users/${username}/packages?page=${page}&per_page=${per_page}`),

    updateProfile: (
        f: FetchFn,
        data: { display_name?: string; bio?: string; website_url?: string }
    ) => request<User>(f, 'PATCH', '/users/me/profile', data)
};

// ── Packages ─────────────────────────────────────────────────────────────────

export const packages = {
    list: (f: FetchFn, page = 1, per_page = 20) =>
        request<Page<Package>>(f, 'GET', `/packages?page=${page}&per_page=${per_page}`),

    get: (f: FetchFn, name: string) => request<Package>(f, 'GET', `/packages/${name}`),

    search: (f: FetchFn, q: string, page = 1, per_page = 20) =>
        request<Page<Package>>(f, 'GET', `/search?q=${encodeURIComponent(q)}&page=${page}&per_page=${per_page}`)
};

// ── Versions ─────────────────────────────────────────────────────────────────

export const versions = {
    list: (f: FetchFn, name: string) =>
        request<PackageVersion[]>(f, 'GET', `/packages/${name}/versions`),

    get: (f: FetchFn, name: string, version: string) =>
        request<PackageVersion>(f, 'GET', `/packages/${name}/versions/${version}`),

    yank: (f: FetchFn, name: string, version: string) =>
        request<void>(f, 'PUT', `/packages/${name}/versions/${version}/yank`),

    unyank: (f: FetchFn, name: string, version: string) =>
        request<void>(f, 'PUT', `/packages/${name}/versions/${version}/unyank`)
};

// ── Owners ───────────────────────────────────────────────────────────────────

export const owners = {
    list: (f: FetchFn, name: string) =>
        request<PackageOwner[]>(f, 'GET', `/packages/${name}/owners`),

    invite: (f: FetchFn, name: string, username: string) =>
        request<{ message: string }>(f, 'POST', `/packages/${name}/owners/invite`, { username }),

    remove: (f: FetchFn, name: string, username: string) =>
        request<void>(f, 'DELETE', `/packages/${name}/owners/${username}`),

    transfer: (f: FetchFn, name: string, to_username: string) =>
        request<{ message: string }>(f, 'POST', `/packages/${name}/transfer`, { to_username })
};

// ── API tokens ───────────────────────────────────────────────────────────────

export const tokens = {
    list: (f: FetchFn) => request<ApiToken[]>(f, 'GET', '/tokens'),

    create: (f: FetchFn, name: string) =>
        request<{ token: string; meta: ApiToken }>(f, 'POST', '/tokens', { name }),

    delete: (f: FetchFn, id: string) => request<void>(f, 'DELETE', `/tokens/${id}`)
};

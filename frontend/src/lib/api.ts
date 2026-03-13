/**
 * Typed API client for the Dal registry backend.
 * All requests go through SvelteKit's fetch so SSR and CSR both work.
 */
import { env } from "$env/dynamic/public";
import type { Cookies } from "@sveltejs/kit";

function normalizeApiBase(value: string | undefined): string | null {
  if (!value) return null;

  const normalized = value.trim().replace(/\/+$/, "");
  return normalized.length > 0 ? normalized : null;
}

export const API_BASE =
  normalizeApiBase(env.PUBLIC_API_BASE) ??
  normalizeApiBase(env.PUBLIC_API_URL) ??
  "https://api.dal.fidan.dev";

// ── Types ────────────────────────────────────────────────────────────────────

export interface User {
  id: string;
  username: string;
  email: string;
  display_name: string | null;
  bio: string | null;
  website_url: string | null;
  avatar_url: string | null;
  email_verified: boolean;
  is_admin: boolean;
  created_at: string;
}

export interface Package {
  id: string;
  name: string;
  description: string | null;
  homepage_url: string | null;
  repository_url: string | null;
  license: string | null;
  readme: string | null;
  keywords: string[];
  categories: string[];
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
  yank_reason: string | null;
  readme: string | null;
  manifest: Record<string, unknown>;
  downloads: number;
  published_at: string;
}

export interface PackageOwner {
  user_id: string;
  username: string;
  display_name: string | null;
  role: "owner" | "collaborator";
  added_at: string;
}

export type TokenScope =
  | "publish:new"
  | "publish:update"
  | "yank"
  | "owner"
  | "user:write";

export interface TokenScopeOption {
  value: TokenScope;
  label: string;
  description: string;
}

export const TOKEN_SCOPE_OPTIONS: TokenScopeOption[] = [
  {
    value: "publish:new",
    label: "Publish new packages",
    description: "Create brand new package names.",
  },
  {
    value: "publish:update",
    label: "Publish updates",
    description: "Upload new versions to packages you already belong to.",
  },
  {
    value: "yank",
    label: "Yank versions",
    description: "Yank and restore published versions.",
  },
  {
    value: "owner",
    label: "Manage owners",
    description: "Invite, remove, and transfer package ownership.",
  },
  {
    value: "user:write",
    label: "Edit profile",
    description: "Update your account profile settings.",
  },
];

export interface ApiToken {
  id: string;
  name: string;
  prefix: string;
  scopes: TokenScope[];
  last_used_at: string | null;
  expires_at: string | null;
  created_at: string;
}

export interface OwnershipInvite {
  id: string;
  package_id: string;
  package_name: string;
  package_description: string | null;
  inviter_username: string;
  inviter_display_name: string | null;
  role: "owner" | "collaborator";
  created_at: string;
  expires_at: string;
  accepted_at: string | null;
  declined_at: string | null;
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

type JsonObject = Record<string, unknown>;

async function request<T>(
  fetchFn: FetchFn,
  method: string,
  path: string,
  body?: unknown,
  options?: { headers?: HeadersInit },
): Promise<T> {
  const headers = new Headers(options?.headers);
  if (body && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }

  const res = await fetchFn(`${API_BASE}${path}`, {
    method,
    credentials: "include",
    headers,
    body: body ? JSON.stringify(body) : undefined,
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
    message: string,
  ) {
    super(message);
    this.name = "DalApiError";
  }
}

export function buildAuthCookieHeader(cookies: Cookies): string | null {
  const accessToken = cookies.get("dal_access_token");
  const refreshToken = cookies.get("dal_refresh_token");
  const cookieHeader = [
    accessToken ? `dal_access_token=${accessToken}` : null,
    refreshToken ? `dal_refresh_token=${refreshToken}` : null,
  ]
    .filter(Boolean)
    .join("; ");

  return cookieHeader.length > 0 ? cookieHeader : null;
}

function asObject(value: unknown): JsonObject {
  return typeof value === "object" && value !== null
    ? (value as JsonObject)
    : {};
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asNullableString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function asBoolean(value: unknown): boolean {
  return value === true;
}

function asNumber(value: unknown): number {
  return typeof value === "number" ? value : 0;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

export function normalizeUser(raw: unknown): User {
  const user = asObject(raw);

  return {
    id: asString(user.id),
    username: asString(user.username),
    email: asString(user.email),
    display_name: asNullableString(user.display_name),
    bio: asNullableString(user.bio),
    website_url:
      asNullableString(user.website_url) ?? asNullableString(user.website),
    avatar_url: asNullableString(user.avatar_url),
    email_verified: asBoolean(user.email_verified),
    is_admin: asBoolean(user.is_admin),
    created_at: asString(user.created_at),
  };
}

function normalizePackage(raw: unknown): Package {
  const pkg = asObject(raw);

  return {
    id: asString(pkg.id),
    name: asString(pkg.name),
    description: asNullableString(pkg.description),
    homepage_url:
      asNullableString(pkg.homepage_url) ?? asNullableString(pkg.homepage),
    repository_url:
      asNullableString(pkg.repository_url) ?? asNullableString(pkg.repository),
    license: asNullableString(pkg.license),
    readme: asNullableString(pkg.readme),
    keywords: asStringArray(pkg.keywords),
    categories: asStringArray(pkg.categories),
    downloads: asNumber(pkg.downloads),
    latest_version: asNullableString(pkg.latest_version),
    updated_at: asString(pkg.updated_at),
    created_at: asString(pkg.created_at, asString(pkg.updated_at)),
  };
}

function normalizeVersion(raw: unknown): PackageVersion {
  const version = asObject(raw);

  return {
    id: asString(version.id),
    package_id: asString(version.package_id),
    version: asString(version.version),
    checksum: asString(version.checksum),
    size_bytes: asNumber(version.size_bytes),
    yanked: asBoolean(version.yanked),
    yank_reason: asNullableString(version.yank_reason),
    readme: asNullableString(version.readme),
    manifest: asObject(version.manifest),
    downloads: asNumber(version.downloads),
    published_at: asString(version.published_at, asString(version.created_at)),
  };
}

function normalizeOwner(raw: unknown): PackageOwner {
  const owner = asObject(raw);

  return {
    user_id: asString(owner.user_id),
    username: asString(owner.username),
    display_name: asNullableString(owner.display_name),
    role: asString(owner.role, "collaborator") as PackageOwner["role"],
    added_at: asString(owner.added_at, asString(owner.created_at)),
  };
}

function normalizeToken(raw: unknown): ApiToken {
  const token = asObject(raw);

  return {
    id: asString(token.id),
    name: asString(token.name),
    prefix: asString(token.prefix),
    scopes: asStringArray(token.scopes) as TokenScope[],
    last_used_at: asNullableString(token.last_used_at),
    expires_at: asNullableString(token.expires_at),
    created_at: asString(token.created_at),
  };
}

function normalizeInvite(raw: unknown): OwnershipInvite {
  const invite = asObject(raw);

  return {
    id: asString(invite.id),
    package_id: asString(invite.package_id),
    package_name: asString(invite.package_name),
    package_description: asNullableString(invite.package_description),
    inviter_username: asString(invite.inviter_username),
    inviter_display_name: asNullableString(invite.inviter_display_name),
    role: asString(invite.role, "collaborator") as OwnershipInvite["role"],
    created_at: asString(invite.created_at),
    expires_at: asString(invite.expires_at),
    accepted_at: asNullableString(invite.accepted_at),
    declined_at: asNullableString(invite.declined_at),
  };
}

function normalizePage<T>(
  raw: unknown,
  mapItem: (item: unknown) => T,
): Page<T> {
  const page = asObject(raw);
  const items = Array.isArray(page.items) ? page.items.map(mapItem) : [];

  return {
    items,
    total: asNumber(page.total),
    page: asNumber(page.page),
    per_page: asNumber(page.per_page),
    pages: asNumber(page.pages),
  };
}

// ── Auth ─────────────────────────────────────────────────────────────────────

export const auth = {
  register: (f: FetchFn, username: string, email: string, password: string) =>
    request<{ message: string }>(f, "POST", "/auth/register", {
      username,
      email,
      password,
    }),

  resendVerification: (f: FetchFn, email: string) =>
    request<{ message: string }>(f, "POST", "/auth/resend-verification", {
      email,
    }),

  resendVerificationByUsername: (f: FetchFn, username: string) =>
    request<{ message: string }>(f, "POST", "/auth/resend-verification", {
      username,
    }),

  login: async (f: FetchFn, username: string, password: string) => {
    const response = await request<unknown>(f, "POST", "/auth/login", {
      username,
      password,
    });
    const payload = asObject(response);

    if (payload.username) {
      return normalizeUser(payload);
    }

    return auth.me(f);
  },

  logout: (f: FetchFn) => request<void>(f, "POST", "/auth/logout"),

  refresh: (f: FetchFn) => request<void>(f, "POST", "/auth/refresh"),

  me: async (f: FetchFn) =>
    normalizeUser(await request<unknown>(f, "GET", "/auth/me")),

  verifyEmail: (f: FetchFn, token: string) =>
    request<{ message: string }>(
      f,
      "GET",
      `/auth/verify-email?token=${encodeURIComponent(token)}`,
    ),

  forgotPassword: (f: FetchFn, email: string) =>
    request<{ message: string }>(f, "POST", "/auth/forgot-password", { email }),

  resetPassword: (f: FetchFn, token: string, new_password: string) =>
    request<{ message: string }>(f, "POST", "/auth/reset-password", {
      token,
      new_password,
    }),
};

// ── Users ────────────────────────────────────────────────────────────────────

export const users = {
  get: async (f: FetchFn, username: string) =>
    normalizeUser(await request<unknown>(f, "GET", `/users/${username}`)),

  packages: async (f: FetchFn, username: string, page = 1, per_page = 20) =>
    normalizePage(
      await request<unknown>(
        f,
        "GET",
        `/users/${username}/packages?page=${page}&per_page=${per_page}`,
      ),
      normalizePackage,
    ),

  updateProfile: (
    f: FetchFn,
    data: { display_name?: string; bio?: string; website_url?: string },
  ) =>
    request<unknown>(f, "PATCH", "/users/me/profile", {
      display_name: data.display_name,
      bio: data.bio,
      website: data.website_url,
    }).then(normalizeUser),
};

// ── Packages ─────────────────────────────────────────────────────────────────

export const packages = {
  list: async (f: FetchFn, page = 1, per_page = 20) =>
    normalizePage(
      await request<unknown>(
        f,
        "GET",
        `/packages?page=${page}&per_page=${per_page}`,
      ),
      normalizePackage,
    ),

  get: async (f: FetchFn, name: string) =>
    normalizePackage(await request<unknown>(f, "GET", `/packages/${name}`)),

  search: async (f: FetchFn, q: string, page = 1, per_page = 20) =>
    normalizePage(
      await request<unknown>(
        f,
        "GET",
        `/search?q=${encodeURIComponent(q)}&page=${page}&per_page=${per_page}`,
      ),
      normalizePackage,
    ),
};

// ── Versions ─────────────────────────────────────────────────────────────────

export const versions = {
  list: async (f: FetchFn, name: string) =>
    (await request<unknown[]>(f, "GET", `/packages/${name}/versions`)).map(
      normalizeVersion,
    ),

  get: async (f: FetchFn, name: string, version: string) =>
    normalizeVersion(
      await request<unknown>(f, "GET", `/packages/${name}/versions/${version}`),
    ),

  yank: (f: FetchFn, name: string, version: string, reason?: string) =>
    request<void>(f, "PUT", `/packages/${name}/versions/${version}/yank`, {
      reason,
    }),

  unyank: (f: FetchFn, name: string, version: string) =>
    request<void>(f, "PUT", `/packages/${name}/versions/${version}/unyank`),
};

// ── Owners ───────────────────────────────────────────────────────────────────

export const owners = {
  list: async (f: FetchFn, name: string) =>
    (await request<unknown[]>(f, "GET", `/packages/${name}/owners`)).map(
      normalizeOwner,
    ),

  invite: (
    f: FetchFn,
    name: string,
    username: string,
    role?: "owner" | "collaborator",
  ) =>
    request<{ message: string }>(f, "POST", `/packages/${name}/owners/invite`, {
      username,
      role,
    }),

  pendingInvites: async (f: FetchFn, options?: { headers?: HeadersInit }) =>
    (
      await request<unknown[]>(f, "GET", "/owners/invites", undefined, options)
    ).map(normalizeInvite),

  acceptInvite: async (f: FetchFn, id: string) => {
    const result = await request<{ invite?: unknown }>(
      f,
      "POST",
      `/owners/invites/${id}/accept`,
    );
    return result.invite ? normalizeInvite(result.invite) : null;
  },

  declineInvite: (f: FetchFn, id: string) =>
    request<{ message: string }>(f, "POST", `/owners/invites/${id}/decline`),

  remove: (f: FetchFn, name: string, username: string) =>
    request<void>(f, "DELETE", `/packages/${name}/owners/${username}`),

  transfer: (f: FetchFn, name: string, to_username: string) =>
    request<{ message: string }>(f, "POST", `/packages/${name}/transfer`, {
      to_username,
    }),
};

// ── API tokens ───────────────────────────────────────────────────────────────

export const tokens = {
  list: async (f: FetchFn, options?: { headers?: HeadersInit }) =>
    (await request<unknown[]>(f, "GET", "/tokens", undefined, options)).map(
      normalizeToken,
    ),

  create: async (
    f: FetchFn,
    name: string,
    options?: { scopes?: TokenScope[]; expires_in?: number | null },
  ) => {
    const created = await request<unknown>(f, "POST", "/tokens", {
      name,
      scopes: options?.scopes,
      expires_in: options?.expires_in ?? undefined,
    });
    const payload = asObject(created);
    const metaSource = payload.meta ?? {
      ...payload,
      scopes: options?.scopes ?? [],
      last_used_at: null,
      expires_at:
        options?.expires_in != null
          ? new Date(Date.now() + options.expires_in * 1000).toISOString()
          : null,
      created_at: new Date().toISOString(),
    };

    return {
      token: asString(payload.token),
      meta: normalizeToken(metaSource),
    };
  },

  delete: (f: FetchFn, id: string) =>
    request<void>(f, "DELETE", `/tokens/${id}`),
};

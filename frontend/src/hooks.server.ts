import { API_BASE, normalizeUser, type User } from "$lib/api";
import type { Handle, RequestEvent } from "@sveltejs/kit";

const ACCESS_COOKIE = "dal_access_token";
const REFRESH_COOKIE = "dal_refresh_token";

export const handle: Handle = async ({ event, resolve }) => {
  event.locals.user = null;

  const accessToken = event.cookies.get(ACCESS_COOKIE);
  const refreshToken = event.cookies.get(REFRESH_COOKIE);
  if (accessToken || refreshToken) {
    let user = await fetchCurrentUser(event.fetch, buildCookieHeader(event));
    if (!user && refreshToken) {
      const refreshed = await refreshSession(event);
      if (refreshed) {
        user = await fetchCurrentUser(event.fetch, buildCookieHeader(event));
      } else {
        clearSessionCookies(event);
      }
    }

    event.locals.user = user;
  }

  return resolve(event);
};

async function fetchCurrentUser(
  fetchFn: typeof fetch,
  cookieHeader: string | null,
): Promise<User | null> {
  if (!cookieHeader) return null;

  try {
    const response = await fetchFn(`${API_BASE}/auth/me`, {
      headers: { cookie: cookieHeader },
    });

    if (!response.ok) {
      return null;
    }

    return normalizeUser(await response.json());
  } catch {
    return null;
  }
}

async function refreshSession(event: RequestEvent): Promise<boolean> {
  const cookieHeader = buildCookieHeader(event);
  if (!cookieHeader) return false;

  try {
    const response = await event.fetch(`${API_BASE}/auth/refresh`, {
      method: "POST",
      headers: { cookie: cookieHeader },
    });

    if (!response.ok) {
      return false;
    }

    applyCookiesFromRefreshResponse(event, response.headers);
    return true;
  } catch {
    return false;
  }
}

function buildCookieHeader(event: RequestEvent): string | null {
  const accessToken = event.cookies.get(ACCESS_COOKIE);
  const refreshToken = event.cookies.get(REFRESH_COOKIE);
  const cookieHeader = [
    accessToken ? `${ACCESS_COOKIE}=${accessToken}` : null,
    refreshToken ? `${REFRESH_COOKIE}=${refreshToken}` : null,
  ]
    .filter(Boolean)
    .join("; ");

  return cookieHeader.length > 0 ? cookieHeader : null;
}

function applyCookiesFromRefreshResponse(event: RequestEvent, headers: Headers) {
  const domain = sharedCookieDomain(event.url.hostname);
  const secure = event.url.protocol === "https:";

  for (const setCookie of getSetCookieValues(headers)) {
    const cookie = parseSetCookie(setCookie);
    if (!cookie) continue;

    event.cookies.set(cookie.name, cookie.value, {
      path: cookie.path ?? "/",
      httpOnly: cookie.httpOnly ?? true,
      secure: cookie.secure ?? secure,
      sameSite: cookie.sameSite ?? "strict",
      maxAge: cookie.maxAge,
      domain,
    });
  }
}

function clearSessionCookies(event: RequestEvent) {
  const domain = sharedCookieDomain(event.url.hostname);

  for (const name of [ACCESS_COOKIE, REFRESH_COOKIE]) {
    event.cookies.set(name, "", {
      path: "/",
      httpOnly: true,
      secure: event.url.protocol === "https:",
      sameSite: "strict",
      maxAge: 0,
      domain,
    });
  }
}

function sharedCookieDomain(hostname: string): string | undefined {
  if (hostname === "localhost") return undefined;
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(hostname)) return undefined;
  return hostname;
}

function getSetCookieValues(headers: Headers): string[] {
  const getSetCookie = (
    headers as Headers & { getSetCookie?: () => string[] }
  ).getSetCookie;
  if (typeof getSetCookie === "function") {
    const values = getSetCookie.call(headers);
    if (values.length > 0) return values;
  }

  const single = headers.get("set-cookie");
  return single ? [single] : [];
}

function parseSetCookie(raw: string) {
  const [pair, ...attributes] = raw.split(";").map((part) => part.trim());
  const [name, ...valueParts] = pair.split("=");
  if (!name || valueParts.length === 0) return null;

  let path: string | undefined;
  let maxAge: number | undefined;
  let httpOnly: boolean | undefined;
  let secure: boolean | undefined;
  let sameSite: "strict" | "lax" | "none" | undefined;

  for (const attribute of attributes) {
    const [rawKey, ...rawValueParts] = attribute.split("=");
    const key = rawKey.toLowerCase();
    const value = rawValueParts.join("=");

    if (key === "path") path = value;
    if (key === "max-age") {
      const parsed = Number.parseInt(value, 10);
      if (!Number.isNaN(parsed)) maxAge = parsed;
    }
    if (key === "httponly") httpOnly = true;
    if (key === "secure") secure = true;
    if (key === "samesite") {
      const normalized = value.toLowerCase();
      if (
        normalized === "strict" ||
        normalized === "lax" ||
        normalized === "none"
      ) {
        sameSite = normalized;
      }
    }
  }

  return {
    name,
    value: valueParts.join("="),
    path,
    maxAge,
    httpOnly,
    secure,
    sameSite,
  };
}

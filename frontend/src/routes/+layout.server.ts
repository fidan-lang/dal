import { API_BASE, normalizeUser } from "$lib/api";
import type { LayoutServerLoad } from "./$types";

export const load: LayoutServerLoad = async ({ fetch, cookies }) => {
  // Only attempt if we have an access_token cookie (set httpOnly by backend)
  const accessToken = cookies.get("dal_access_token");
  const refreshToken = cookies.get("dal_refresh_token");
  if (!accessToken && !refreshToken) return { user: null };

  try {
    const cookieHeader = [
      accessToken ? `dal_access_token=${accessToken}` : null,
      refreshToken ? `dal_refresh_token=${refreshToken}` : null,
    ]
      .filter(Boolean)
      .join("; ");

    const response = await fetch(`${API_BASE}/auth/me`, {
      headers: {
        cookie: cookieHeader,
      },
    });

    if (!response.ok) {
      return { user: null };
    }

    const user = normalizeUser(await response.json());
    return { user };
  } catch {
    return { user: null };
  }
};

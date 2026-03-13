import { buildAuthCookieHeader, owners, users } from "$lib/api";
import type { PageServerLoad } from "./$types";

export const load: PageServerLoad = async ({ fetch, cookies, parent }) => {
  const { user } = await parent();
  if (!user)
    return {
      packages: { items: [], total: 0, page: 1, per_page: 20, pages: 0 },
      pendingInvites: [],
    };
  const cookieHeader = buildAuthCookieHeader(cookies);
  const [pkgs, pendingInvites] = await Promise.all([
    users.packages(fetch, user.username, 1, 50),
    owners.pendingInvites(fetch, {
      headers: cookieHeader ? { cookie: cookieHeader } : undefined,
    }),
  ]);
  return { packages: pkgs, pendingInvites };
};

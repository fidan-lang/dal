import { buildAuthCookieHeader, tokens } from "$lib/api";
import type { PageServerLoad } from "./$types";

export const load: PageServerLoad = async ({ fetch, cookies }) => {
  const cookieHeader = buildAuthCookieHeader(cookies);
  const list = await tokens.list(fetch, {
    headers: cookieHeader ? { cookie: cookieHeader } : undefined,
  });
  return { tokens: list };
};

import { packages } from "$lib/api";
import type { PageServerLoad } from "./$types";

export const load: PageServerLoad = async ({ fetch, url }) => {
  const page = Math.max(1, Number(url.searchParams.get("page") ?? "1"));
  const per_page = 24;
  try {
    const data = await packages.list(fetch, page, per_page);
    return { ...data, page, per_page };
  } catch {
    return { items: [], total: 0, pages: 0, page, per_page };
  }
};

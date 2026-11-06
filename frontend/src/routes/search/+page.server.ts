import { packages } from "$lib/api";
import type { PageServerLoad } from "./$types";

export const load: PageServerLoad = async ({ fetch, url }) => {
  const q = (url.searchParams.get("q") ?? "").trim();
  const category = (url.searchParams.get("category") ?? "").trim();
  const page = Math.max(1, Number(url.searchParams.get("page") ?? "1"));
  const per_page = 20;
  if (!q && !category)
    return { q, category, items: [], total: 0, page: 1, per_page, pages: 0 };
  try {
    const data = await packages.search(fetch, q, page, per_page, category);
    return { q, category, ...data, page, per_page };
  } catch {
    return { q, category, items: [], total: 0, page, per_page, pages: 0 };
  }
};

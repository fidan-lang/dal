import { packages } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, url }) => {
    const q = url.searchParams.get('q') ?? '';
    const page = Math.max(1, Number(url.searchParams.get('page') ?? '1'));
    const per_page = 20;
    if (!q) return { q, items: [], total: 0, page: 1, per_page, pages: 0 };
    try {
        const data = await packages.search(fetch, q, page, per_page);
        return { q, ...data, page, per_page };
    } catch {
        return { q, items: [], total: 0, page, per_page, pages: 0 };
    }
};

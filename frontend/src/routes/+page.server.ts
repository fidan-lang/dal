import { packages } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch }) => {
    try {
        const recent = await packages.list(fetch, 1, 8);
        return { recent };
    } catch {
        return { recent: { items: [], total: 0, page: 1, per_page: 8, pages: 0 } };
    }
};

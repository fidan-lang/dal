import { users } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, parent }) => {
    const { user } = await parent();
    if (!user) return { packages: { items: [], total: 0, page: 1, per_page: 20, pages: 0 } };
    const pkgs = await users.packages(fetch, user.username, 1, 50);
    return { packages: pkgs };
};

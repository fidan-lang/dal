import { users } from '$lib/api';
import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params }) => {
    try {
        const [user, pkgs] = await Promise.all([
            users.get(fetch, params.username),
            users.packages(fetch, params.username, 1, 20)
        ]);
        return { user, packages: pkgs };
    } catch {
        error(404, 'User not found');
    }
};

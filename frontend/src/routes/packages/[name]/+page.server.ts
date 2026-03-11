import { packages as api, versions } from '$lib/api';
import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params }) => {
    try {
        const [pkg, versionList] = await Promise.all([
            api.get(fetch, params.name),
            versions.list(fetch, params.name)
        ]);
        return { pkg, versions: versionList };
    } catch {
        error(404, 'Package not found');
    }
};

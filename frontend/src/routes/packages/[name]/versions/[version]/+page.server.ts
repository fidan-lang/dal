import { versions as versionsApi } from '$lib/api';
import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params }) => {
    try {
        const version = await versionsApi.get(fetch, params.name, params.version);
        return { version, pkgName: params.name };
    } catch {
        error(404, 'Version not found');
    }
};

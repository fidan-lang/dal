import { auth } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ url, fetch }) => {
    const token = url.searchParams.get('token') ?? '';
    if (!token) return { status: 'missing' as const };
    try {
        await auth.verifyEmail(fetch, token);
        return { status: 'ok' as const };
    } catch {
        return { status: 'invalid' as const };
    }
};

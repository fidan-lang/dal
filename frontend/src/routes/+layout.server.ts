import { auth } from '$lib/api';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ fetch, cookies }) => {
    // Only attempt if we have an access_token cookie (set httpOnly by backend)
    const hasSession = cookies.get('access_token') || cookies.get('refresh_token');
    if (!hasSession) return { user: null };

    try {
        const user = await auth.me(fetch);
        return { user };
    } catch {
        return { user: null };
    }
};

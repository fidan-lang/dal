import { tokens } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch }) => {
    const list = await tokens.list(fetch);
    return { tokens: list };
};

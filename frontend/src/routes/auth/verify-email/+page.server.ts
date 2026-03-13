import { redirect } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";

export const load: PageServerLoad = async ({ url }) => {
  const suffix = url.search ? url.search : "";
  throw redirect(307, `/verify-email${suffix}`);
};

import { owners, packages, versions } from "$lib/api";
import { error } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";

export const load: PageServerLoad = async ({ fetch, params, parent }) => {
  const { user } = await parent();
  if (!user) {
    error(401, "Authentication required");
  }

  try {
    const [pkg, versionList, ownerList] = await Promise.all([
      packages.get(fetch, params.name),
      versions.list(fetch, params.name),
      owners.list(fetch, params.name),
    ]);

    const membership = ownerList.find((owner) => owner.username === user.username);
    if (!membership) {
      error(403, "You do not manage this package");
    }

    return {
      pkg,
      versions: versionList,
      owners: ownerList,
      currentUsername: user.username,
      currentRole: membership.role,
    };
  } catch (err) {
    if (err instanceof Response) {
      throw err;
    }

    error(404, "Package not found");
  }
};

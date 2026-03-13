export const onRequest: PagesFunction = ({ request, next }) => {
  const host = new URL(request.url).hostname;

  if (host.endsWith(".pages.dev")) {
    return new Response("Forbidden", { status: 403 });
  }

  return next();
};

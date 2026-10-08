# MyGo website

The home page and, at `/docs`, the guides of [`../docs`](../docs), built with
TanStack Start and shadcn/ui. Every page is prerendered at build time, and
Cloudflare Workers serves the result as static assets, with no Worker code.

```sh
bun install                    # at the root of the repository
bun run --cwd website dev      # http://localhost:3000
bun run --cwd website build    # dist/client
bun run --cwd website preview  # dist/client, served by wrangler as in production
bun run --cwd website deploy   # build, then wrangler deploy
```

Pages of the docs are rendered from the markdown when the site is built:
the order of the sidebar comes from the lists of `docs/README.md`, pages in
directories keep their path (`plugins/fetch.md` is `/docs/plugins/fetch`),
and links between pages, such as `bindings.md#events`, become links of the
site. Lists of pages, `- [Title](page.md): what it covers`, become cards.

Native UI's docs, in `docs/ui`, have a sidebar of their own, from the lists
of `docs/ui/README.md` (the page at `/docs/ui`), and the header's UI link;
the docs' sidebar leaves them out. A directory's `README.md` is the page of
the directory.

# sv

Everything you need to build a Svelte project, powered by [`sv`](https://github.com/sveltejs/cli).

## Creating a project

If you're seeing this, you've probably already done this step. Congrats!

```sh
# create a new project
npx sv create my-app
```

To recreate this project with the same configuration:

```sh
# recreate this project
pnpm dlx sv@0.15.1 create --template minimal --types ts --add tailwindcss="plugins:none" mcp="ide:opencode" --install pnpm ./web
```

## Developing

Once you've created a project and installed dependencies with `npm install` (or `pnpm install` or `yarn`), start a development server:

```sh
npm run dev

# or start the server and open the app in a new browser tab
npm run dev -- --open
```

The Rust server sends no CORS headers, so the browser cannot call it from the Vite
dev server origin directly. The dev server therefore forwards the paths the app
actually calls (`/healthz` and `/api/*`) to the Rust server, leaving the paths
unchanged. That way an empty backend URL means "current origin" in development
exactly as it does in production, where the Rust server serves both the built app
and the API from one origin.

Default proxy target:

```sh
http://127.0.0.1:8787
```

Override it if your backend listens elsewhere:

```sh
VITE_BACKEND_TARGET=http://127.0.0.1:9000 pnpm dev
```

## Building

To create a production version of your app:

```sh
npm run build
```

You can preview the production build with `npm run preview`.

> To deploy your app, you may need to install an [adapter](https://svelte.dev/docs/kit/adapters) for your target environment.

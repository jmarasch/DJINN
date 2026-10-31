import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte()],
  build: {
    // TVs with Chromecast built in run old Chrome (84 on a 2016 Vizio), and
    // the TV page runs on them. Newer syntax is rewritten for them.
    target: ['chrome80', 'safari14', 'firefox78', 'edge88'],
    // The party app, and DJINN GO (served on its own port).
    rollupOptions: {
      input: {
        index: 'index.html',
        go: 'go.html',
      },
    },
  },
  server: {
    // `npm run dev` talks to a locally running `djinn` binary.
    proxy: {
      '/api': 'http://localhost:8066',
    },
  },
})

import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte()],
  server: {
    // `npm run dev` talks to a locally running `djinn` binary.
    proxy: {
      '/api': 'http://localhost:8066',
    },
  },
})

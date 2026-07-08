// Copyright © 2026 Jalapeno Labs

import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react-swc'

// The SPA, its API, and its auth routes are served from ONE origin: the dev
// server proxies `/api` and `/auth` to the backend. This keeps the session cookie
// first-party (no SameSite=None, no CORS) and mirrors the production reverse-proxy
// topology. Inside Compose the backend is reachable as `backend:31416`.
const proxyTarget = process.env.API_PROXY_TARGET || 'http://localhost:31416'

// https://vite.dev/config/
export default defineConfig({
  plugins: [ react() ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    // Bind on all interfaces so the container's dev server is reachable
    // from the host browser.
    host: true,
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: proxyTarget, changeOrigin: true },
      '/auth': { target: proxyTarget, changeOrigin: true },
    },
    watch: {
      // Bind-mounted volumes on Windows/Docker don't emit reliable inotify
      // events, so poll for changes to keep HMR working inside the container.
      usePolling: true,
      interval: 100,
    },
  },
})

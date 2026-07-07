// Copyright © 2026 Jalapeno Labs

import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react-swc'

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
    watch: {
      // Bind-mounted volumes on Windows/Docker don't emit reliable inotify
      // events, so poll for changes to keep HMR working inside the container.
      usePolling: true,
      interval: 100,
    },
  },
})

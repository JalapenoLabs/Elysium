// Copyright © 2026 Jalapeno Labs

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

// When the dev server runs behind nginx (compose), the browser reaches the HMR
// websocket through nginx's published port rather than Vite's own port.
// Compose sets this; a bare `yarn dev` leaves it unset and Vite uses its default.
const hmrClientPort = Number(process.env.VITE_HMR_CLIENT_PORT) || undefined

// https://vite.dev/config/
export default defineConfig({
  plugins: [ react(), tailwindcss() ],
  server: {
    host: true,
    port: 5173,
    strictPort: true,
    hmr: {
      clientPort: hmrClientPort,
    },
    // Outside compose there is no nginx in front, so forward API calls straight
    // to a locally running API. Inside compose nginx routes /api before Vite sees it.
    proxy: {
      '/api': {
        target: process.env.VITE_API_PROXY_TARGET || 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
  // Unit tests for slices, selectors, helpers, and components. jsdom gives component
  // tests a DOM, and the setup file loads the translations components read.
  test: {
    environment: 'jsdom',
    setupFiles: [ './src/testSetup.ts' ],
  },
})

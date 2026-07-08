// Copyright © 2026 Jalapeno Labs

// Utility
import { resolve } from 'node:path'

// Where the shell loads the app in development: the dockerized stack's web
// frontend (Vite dev server today). Override with ELYSIUM_APP_URL to target a
// remote or differently-ported host.
export const DEFAULT_APP_URL = 'http://localhost:5173'
export const APP_URL = process.env.ELYSIUM_APP_URL || DEFAULT_APP_URL

// Development runs unpackaged (yarn dev / electronmon); an installed build runs
// from inside the packaged app.asar archive. `app.isPackaged` is not fully
// reliable, so detect dev by whether this module resolves inside app.asar. This
// decides whether the renderer is loaded from the dev URL or the packaged build.
export const IS_DEV = !import.meta.url.includes('app.asar')

// In production the built frontend SPA is served locally over a custom protocol
// by electron-serve from this directory. Populated by the frontend build during
// packaging (see docs/desktop.md).
export const RENDERER_DIRECTORY = resolve(import.meta.dirname, '..', 'renderer')

// The preload script, resolved next to the compiled main output. It is an ESM
// (.mjs) preload, which Electron requires for ES-module preload scripts.
export const PRELOAD_SCRIPT = resolve(import.meta.dirname, 'preload.mjs')

// How long to wait before retrying a failed app load. In dev the stack may still
// be starting; the frontend owns all on-screen UI, so the shell simply keeps
// trying until it can hand off rather than rendering its own page.
export const APP_RETRY_DELAY_MS = 2000

// Initial window size. Deliberately roomy: the primary view is a full Claude
// session workspace, not a compact utility window.
export const WINDOW_WIDTH = 1440
export const WINDOW_HEIGHT = 900
export const WINDOW_MIN_WIDTH = 960
export const WINDOW_MIN_HEIGHT = 640

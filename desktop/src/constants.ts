// Copyright © 2026 Jalapeno Labs

import { join } from 'node:path'

// Where the shell loads the Elysium app from. The web app is served by the
// dockerized stack (Vite dev server today, a production static server later);
// the desktop shell never bundles the app, it only points a window at it.
//
// Override with ELYSIUM_APP_URL to target a remote or differently-ported host.
export const DEFAULT_APP_URL = 'http://localhost:5173'
export const APP_URL = process.env.ELYSIUM_APP_URL || DEFAULT_APP_URL

// Local fallback page shown when the app URL is unreachable (e.g. the stack is
// not up yet). It offers a retry rather than a blank window. Bundled alongside
// the compiled output via electron-builder's `files` globs.
export const LAUNCHER_PAGE = join(__dirname, '..', 'static', 'launcher.html')
export const PRELOAD_SCRIPT = join(__dirname, 'preload.js')

// Initial window size. Deliberately roomy: the primary view is a full Claude
// session workspace, not a compact utility window.
export const WINDOW_WIDTH = 1440
export const WINDOW_HEIGHT = 900
export const WINDOW_MIN_WIDTH = 960
export const WINDOW_MIN_HEIGHT = 640

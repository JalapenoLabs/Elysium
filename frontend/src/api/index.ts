// Core
import ky from 'ky'

// Misc
import { API_BASE_URL } from '../constants'

// Shared Ky instance. Route modules under `./routes` build on top of this.
export const apiClient = ky.create({
  // Ky 2.0 uses the web-standard `baseUrl`; relative inputs like 'api/v1/hello'
  // resolve against it.
  baseUrl: API_BASE_URL,
  // Local dev talks to a single API instance; skip retries so failures surface
  // immediately instead of stalling the UI.
  retry: 0,
})

// Copyright © 2026 Jalapeno Labs

// The API and auth routes are served same-origin (the dev server / production
// gateway proxies `/api` and `/auth` to the backend), so the client just uses the
// current origin. No cross-origin base URL, no CORS.
export const API_BASE_URL = window.location.origin

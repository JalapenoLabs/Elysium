// Copyright © 2026 Jalapeno Labs

// nginx routes everything under this path to the Rust API on the same origin.
export const API_BASE_PATH = '/api'

// Generous enough for cold starts, short enough that a hung request fails visibly.
export const API_REQUEST_TIMEOUT_MS = 10_000

// The API gives a satellite 10 seconds to replay a thread's history; this leaves room
// for that answer to arrive.
export const SESSION_HISTORY_TIMEOUT_MS = 15_000

// Mail server setup waits up to 20 seconds for Stalwart to restart, inside the API's
// 30-second request timeout; this outlasts both, so the API's own answer arrives.
export const MAIL_SERVER_SETUP_TIMEOUT_MS = 35_000

// The one server-sent event stream every page keeps open.
export const EVENT_STREAM_PATH = `${API_BASE_PATH}/v1/events`

// Browsers retry a dropped EventSource on their own, but give up for good after an
// HTTP error. Elysium then reopens it, backing off between these bounds.
export const EVENT_STREAM_RETRY_INITIAL_MS = 1_000
export const EVENT_STREAM_RETRY_MAX_MS = 30_000

// Live events kept per open conversation. Older ones drop off the front; reopening
// the conversation fetches history again.
export const SESSION_EVENTS_LIMIT = 5_000

// Versioned so an incompatible layout from an older build is ignored, not restored.
export const CODING_LAYOUT_STORAGE_KEY = 'elysium.coding.layout.v1'

// Which view the Projects page last showed, remembered per browser.
export const PROJECTS_VIEW_STORAGE_KEY = 'elysium.projects.view.v1'

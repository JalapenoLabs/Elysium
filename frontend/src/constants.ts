// Copyright © 2026 Jalapeno Labs

// nginx routes everything under this path to the Rust API on the same origin.
export const API_BASE_PATH = '/api'

// Generous enough for cold starts, short enough that a hung request fails visibly.
export const API_REQUEST_TIMEOUT_MS = 10_000

// The API gives a satellite 10 seconds to replay a thread's history; this leaves room
// for that answer to arrive.
export const SESSION_HISTORY_TIMEOUT_MS = 15_000

// A DNS check looks up every record at once, and a resolver that does not answer takes
// about ten seconds per lookup to give up; this leaves room for that answer to arrive.
export const MAIL_DNS_CHECK_TIMEOUT_MS = 20_000

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
export const CODING_LAYOUT_STORAGE_KEY = 'elysium.coding.layout.v2'

// Whether the Coding page's Sessions panel shows a table or tiles, remembered per browser.
export const CODING_SESSIONS_VIEW_STORAGE_KEY = 'elysium.coding.sessions.view.v1'

// Project cover uploads: the API refuses anything larger, and reads the format from the
// bytes. These let the form refuse early and the file picker offer only images.
export const PROJECT_COVER_MAX_BYTES = 10_000_000
export const PROJECT_COVER_ACCEPTED_TYPES = [ 'image/png', 'image/jpeg', 'image/webp', 'image/gif' ]

// Sending 10 MB and compressing it can outlast the default request timeout on a slow link.
export const PROJECT_COVER_UPLOAD_TIMEOUT_MS = 60_000

// How long the pointer rests on an ImagePreview before the larger image appears. Long
// enough that sweeping across a table of thumbnails opens nothing.
export const IMAGE_PREVIEW_HOVER_DELAY_MS = 2_000

// Storage limits are entered and shown in decimal units, as storage providers bill them.
export const BYTES_PER_GIGABYTE = 1_000_000_000

// Which view the Projects page last showed, remembered per browser.
export const PROJECTS_VIEW_STORAGE_KEY = 'elysium.projects.view.v1'

// Snoozing an item to a day wakes it at this hour in the viewer's zone, and "later today"
// snoozes it this many hours from now.
export const SNOOZE_WAKE_HOUR = 9
export const SNOOZE_LATER_TODAY_HOURS = 3

// How often views that depend on the time (Next, overdue and snoozed markers) look at the
// clock again, so a snooze that runs out brings its item back. No request is made.
export const ACTION_ITEMS_CLOCK_TICK_MS = 60_000

// A waiting-on name or address and an owner's name, matching the API's limit.
export const ACTION_ITEM_PERSON_MAX_CHARACTERS = 320

// How long typing in a link picker's search rests before Jira is asked, so a word typed
// quickly is one search rather than one per letter.
export const LINK_SEARCH_DELAY_MS = 350

// How many issues one link picker search lists. A person picks from the first screen; a
// narrower search finds the rest.
export const LINK_SEARCH_RESULTS = 25

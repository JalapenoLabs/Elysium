// Copyright © 2026 Jalapeno Labs

// nginx routes everything under this path to the Rust API on the same origin.
export const API_BASE_PATH = '/api'

// Generous enough for cold starts, short enough that a hung request fails visibly.
export const API_REQUEST_TIMEOUT_MS = 10_000

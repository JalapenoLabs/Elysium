// Copyright © 2026 Jalapeno Labs

// Core
import ky from 'ky'

// Misc
import { API_BASE_PATH, API_REQUEST_TIMEOUT_MS } from '../constants'

// Single shared HTTP client. Every request goes through nginx on the page's own
// origin, so the prefix is an origin-relative path rather than a host.
export const apiClient = ky.create({
  prefix: API_BASE_PATH,
  timeout: API_REQUEST_TIMEOUT_MS,
  retry: {
    limit: 2,
    methods: [ 'get' ],
  },
  headers: {
    Accept: 'application/json, text/plain',
  },
})

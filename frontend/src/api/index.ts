// Copyright © 2026 Jalapeno Labs

// Core
import ky from 'ky'

// Redux
import { store } from '../store'
import { accessRefused, isAccessRefusalCode } from '../store/authSlice'

// Misc
import { API_BASE_PATH, API_REQUEST_TIMEOUT_MS } from '../constants'

// Single shared HTTP client. Every request goes through nginx on the page's own
// origin, so the prefix is an origin-relative path rather than a host, and the session
// cookie Kratos set travels with it.
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
  hooks: {
    afterResponse: [
      // Any request can learn the session ended or the account changed. The refusal lands in
      // Redux, and `AuthGate` sends the person where it leads; the request still fails.
      async ({ response }) => {
        if (response.status !== 401 && response.status !== 403) {
          return
        }

        const body: unknown = await response.clone().json().catch(() => null)
        const code = typeof body === 'object' && body !== null && 'code' in body
          ? body.code
          : null
        if (isAccessRefusalCode(code)) {
          store.dispatch(accessRefused(code))
        }
      },
    ],
  },
})

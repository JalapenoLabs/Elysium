// Copyright © 2026 Jalapeno Labs

// Core
import Keycloak from 'keycloak-js'

// Misc
import { KEYCLOAK_URL, KEYCLOAK_REALM, KEYCLOAK_CLIENT_ID } from '../constants'

// Single Keycloak instance for the whole app. Keycloak owns every credential and
// login ceremony (passkey, TOTP); the SPA only redirects to it via Authorization
// Code + PKCE and carries the resulting token.
export const keycloak = new Keycloak({
  url: KEYCLOAK_URL,
  realm: KEYCLOAK_REALM,
  clientId: KEYCLOAK_CLIENT_ID,
})

// `init` may only be called once per instance; guard so hot reloads don't retry.
let initialized = false

// Initializes Keycloak with a silent SSO check (no forced login) and PKCE.
//
// Returns whether an existing session was found. Failures (e.g. Keycloak briefly
// unavailable) are surfaced to the caller, which renders the app unauthenticated
// rather than blocking on the identity provider.
export async function initKeycloak(): Promise<boolean> {
  if (initialized) {
    return keycloak.authenticated ?? false
  }
  initialized = true

  return keycloak.init({
    onLoad: 'check-sso',
    pkceMethod: 'S256',
    silentCheckSsoRedirectUri: `${window.location.origin}/silent-check-sso.html`,
    // The login-status iframe is unreliable across origins in dev; refreshing the
    // token on demand (see the Ky hook) covers session expiry instead.
    checkLoginIframe: false,
  })
}

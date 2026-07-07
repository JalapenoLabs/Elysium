// Copyright © 2026 Jalapeno Labs

// Misc
import { keycloak } from './keycloak'

// Exposes the current auth state plus login/logout actions.
//
// This intentionally reads the Keycloak instance directly rather than tracking
// state in React: `login` and `logout` navigate the whole page, so the app
// reloads and re-reads fresh state on return; no reactive subscription is needed.
export function useAuth() {
  return {
    authenticated: keycloak.authenticated ?? false,
    username: keycloak.tokenParsed?.preferred_username ?? '',
    login: () => keycloak.login(),
    logout: () => keycloak.logout({ redirectUri: window.location.origin }),
  } as const
}

// Copyright © 2026 Jalapeno Labs

// Base URL for the Rust API. Injected at build time by Vite; falls back to the
// port the backend publishes locally so the template runs with zero config.
export const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || 'http://localhost:31416'

// Keycloak OIDC endpoints, injected at build time by Vite (see compose.yaml).
// Keycloak owns every credential and login ceremony; the SPA only redirects to it.
export const KEYCLOAK_URL = import.meta.env.VITE_KEYCLOAK_URL || 'http://localhost:8080'
export const KEYCLOAK_REALM = import.meta.env.VITE_KEYCLOAK_REALM || 'elysium'
export const KEYCLOAK_CLIENT_ID = import.meta.env.VITE_KEYCLOAK_CLIENT_ID || 'elysium-frontend'

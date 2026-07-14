// Copyright © 2026 Jalapeno Labs

// The auth methods Elysium can offer, keyed to the server's method strings
// (`POST /api/v1/auth/methods`). New methods slot in here and the registry picks
// them up; see docs/auth-model.md.
export type AuthMethodKind = 'passkey' | 'password' | 'magic_link'

// Copyright © 2026 Jalapeno Labs

import type { AuthMethodKind } from './types'

// Presentational metadata for an auth method: the i18n keys a chooser renders.
// Kept free of hooks so it never couples the registry to a flow; the UI picks the
// right hook by kind. New methods are added here and the chooser iterates them.
type AuthMethodMeta = {
  kind: AuthMethodKind
  titleKey: string
  blurbKey: string
}

// The one source of truth mapping a method kind to how it presents. A discovery
// response (`AuthMethodKind[]`) maps straight through this to render cards.
export const authMethodByKind = {
  passkey: {
    kind: 'passkey',
    titleKey: 'method.passkey.title',
    blurbKey: 'method.passkey.blurb',
  },
  password: {
    kind: 'password',
    titleKey: 'method.password.title',
    blurbKey: 'method.password.blurb',
  },
  magic_link: {
    kind: 'magic_link',
    titleKey: 'method.magicLink.title',
    blurbKey: 'method.magicLink.blurb',
  },
} as const satisfies Record<AuthMethodKind, AuthMethodMeta>

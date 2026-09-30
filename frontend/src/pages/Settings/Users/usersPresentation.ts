// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { PersonRole, User, UserRole, UserStatus } from '../../../api/routes/userRoutes'

// The roles an admin can give a person, most access first.
export const PERSON_ROLES = [ 'admin', 'member', 'guest' ] as const satisfies readonly PersonRole[]

export const roleLabelKeys = {
  admin: 'roles.admin',
  member: 'roles.member',
  guest: 'roles.guest',
  agent: 'roles.agent',
  system: 'roles.system',
} as const satisfies Record<UserRole, ParseKeys<'users'>>

export const statusLabelKeys = {
  pending: 'statuses.pending',
  active: 'statuses.active',
  disabled: 'statuses.disabled',
} as const satisfies Record<UserStatus, ParseKeys<'users'>>

export const statusChipColors = {
  pending: 'warning',
  active: 'success',
  disabled: 'danger',
} as const satisfies Record<UserStatus, 'warning' | 'success' | 'danger'>

// How someone can sign in, as Kratos names each kind of credential.
const signInMethodLabelKeys = new Map<string, ParseKeys<'users'>>([
  [ 'password', 'methods.password' ],
  [ 'passkey', 'methods.passkey' ],
  [ 'totp', 'methods.totp' ],
  [ 'lookup_secret', 'methods.lookupSecret' ],
])

// Each sign-in method a person has, as a translation key, or as Kratos names it when this
// build does not know it.
export function describeSignInMethods(methods: string[] | undefined) {
  return (methods ?? []).map((method) => {
    const key = signInMethodLabelKeys.get(method)
    if (!key) {
      console.debug('describeSignInMethods received a method this build does not know', { method })
    }
    return { method, key: key ?? null } as const
  })
}

// Whether a person has a second factor: an authenticator app.
export function hasAuthenticator(user: User) {
  return Boolean(user.signInMethods?.includes('totp'))
}

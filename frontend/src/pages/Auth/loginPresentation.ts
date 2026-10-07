// Copyright © 2026 Jalapeno Labs

import type { LoginFlow, UpdateLoginFlowBody } from '@ory/client-fetch'
import type { ParseKeys } from 'i18next'

// The ways Elysium signs people in, as Kratos takes each submission.
export type LoginSubmission = Extract<
  UpdateLoginFlowBody,
  { method: 'password' | 'passkey' | 'totp' | 'lookup_secret' }
>

// What a sign-in flow asks for: a first sign-in, a recent one confirmed before a security
// change, or the second factor after a password or passkey.
export type LoginMode = 'signIn' | 'refresh' | 'secondFactor'

export function getLoginMode(flow: Pick<LoginFlow, 'requested_aal' | 'refresh'> | null): LoginMode {
  if (flow?.requested_aal === 'aal2') {
    return 'secondFactor'
  }
  if (flow?.refresh) {
    return 'refresh'
  }
  return 'signIn'
}

export const loginCopyByMode = {
  signIn: { heading: 'login.heading', description: 'login.description' },
  refresh: { heading: 'login.refresh.heading', description: 'login.refresh.description' },
  secondFactor: { heading: 'login.secondFactor.heading', description: 'login.secondFactor.description' },
} as const satisfies Record<LoginMode, { heading: ParseKeys<'auth'>, description: ParseKeys<'auth'> }>

// Kratos answers a password sign-in on an account with an authenticator app by redirecting
// to its own endpoint for a second-factor flow. The sign-in page asks for the code itself,
// keeping where the person was headed.
export function isSecondFactorRedirect(url: string): boolean {
  try {
    const target = new URL(url, window.location.origin)
    return target.pathname.endsWith('/self-service/login/browser') && target.searchParams.get('aal') === 'aal2'
  }
  catch (error) {
    console.debug('isSecondFactorRedirect received something that is not a URL', { url, error })
    return false
  }
}

// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { accessRefused, isAccessRefusalCode, meLoaded, selectAccessRefusal, selectIsAdmin, selectMe } from './authSlice'
import { userUpserted } from './usersSlice'

// Misc
import { makeUser } from '../testFixtures'

function me(overrides: Parameters<typeof makeUser>[0] = {}) {
  return {
    user: makeUser(overrides),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
  }
}

describe('authSlice', () => {
  it('clears a refusal once the API answers who is signed in', () => {
    const store = createAppStore()
    store.dispatch(accessRefused('unauthenticated'))
    expect(selectAccessRefusal(store.getState())).toBe('unauthenticated')

    store.dispatch(meLoaded(me()))
    expect(selectAccessRefusal(store.getState())).toBeNull()
    expect(selectMe(store.getState())?.user.name).toBe('Ada Lovelace')
  })

  it('takes an admin change to this person from the event stream', () => {
    const store = createAppStore()
    store.dispatch(meLoaded(me({ id: 'ada', role: 'member' })))
    store.dispatch(userUpserted(makeUser({ id: 'ada', role: 'admin' })))
    expect(selectMe(store.getState())?.user.role).toBe('admin')

    // Someone else's change leaves this person as they are.
    store.dispatch(userUpserted(makeUser({ id: 'grace', role: 'guest' })))
    expect(selectMe(store.getState())?.user.id).toBe('ada')
  })
})

describe('selectIsAdmin', () => {
  it('is true only for an active admin', () => {
    const store = createAppStore()
    expect(selectIsAdmin(store.getState())).toBe(false)

    store.dispatch(meLoaded(me({ role: 'admin', status: 'active' })))
    expect(selectIsAdmin(store.getState())).toBe(true)

    store.dispatch(meLoaded(me({ role: 'admin', status: 'disabled' })))
    expect(selectIsAdmin(store.getState())).toBe(false)

    store.dispatch(meLoaded(me({ role: 'member', status: 'active' })))
    expect(selectIsAdmin(store.getState())).toBe(false)
  })
})

describe('isAccessRefusalCode', () => {
  it('knows the codes that send someone elsewhere, and nothing else', () => {
    expect(isAccessRefusalCode('account_pending')).toBe(true)
    expect(isAccessRefusalCode('mfa_enrollment_required')).toBe(true)
    expect(isAccessRefusalCode('admin_only')).toBe(false)
    expect(isAccessRefusalCode(undefined)).toBe(false)
  })
})

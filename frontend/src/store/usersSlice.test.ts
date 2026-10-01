// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { selectPeopleByStanding, selectUserNamesById, userDeleted, usersLoaded, userUpserted } from './usersSlice'

// Misc
import { makeUser } from '../testFixtures'

const system = makeUser({
  id: 'system',
  kind: 'machine',
  name: 'Elysium',
  email: null,
  role: 'system',
  status: null,
})

describe('selectPeopleByStanding', () => {
  it('puts sign-ups waiting first and leaves machines out', () => {
    const store = createAppStore()
    store.dispatch(usersLoaded([
      system,
      makeUser({ id: 'ada', name: 'Ada', status: 'active' }),
      makeUser({ id: 'grace', name: 'Grace', status: 'pending', role: null }),
      makeUser({ id: 'linus', name: 'Linus', status: 'disabled' }),
    ]))

    const people = selectPeopleByStanding(store.getState())
    expect(people.pending.map((user) => user.id)).toEqual([ 'grace' ])
    expect(people.approved.map((user) => user.id)).toEqual([ 'ada', 'linus' ])
  })

  it('moves an approved sign-up out of the waiting list, and drops a rejected one', () => {
    const store = createAppStore()
    store.dispatch(usersLoaded([
      makeUser({ id: 'grace', name: 'Grace', status: 'pending', role: null }),
      makeUser({ id: 'mallory', name: 'Mallory', status: 'pending', role: null }),
    ]))
    store.dispatch(userUpserted(makeUser({ id: 'grace', name: 'Grace', status: 'active', role: 'member' })))
    store.dispatch(userDeleted('mallory'))

    const people = selectPeopleByStanding(store.getState())
    expect(people.pending).toEqual([])
    expect(people.approved.map((user) => user.id)).toEqual([ 'grace' ])
  })
})

describe('selectUserNamesById', () => {
  it('names machines and people alike', () => {
    const store = createAppStore()
    store.dispatch(usersLoaded([ system, makeUser({ id: 'ada', name: 'Ada' }) ]))
    expect(selectUserNamesById(store.getState())).toEqual({ system: 'Elysium', ada: 'Ada' })
  })
})

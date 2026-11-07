// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getLeadAfter, getMembershipChanges } from './membershipChanges'

describe('getMembershipChanges', () => {
  it('names what joined and what left', () => {
    expect(getMembershipChanges([ 'kept', 'left' ], [ 'kept', 'joined' ])).toEqual({
      added: [ 'joined' ],
      removed: [ 'left' ],
    })
  })

  it('changes nothing when the selection is the same in another order', () => {
    expect(getMembershipChanges([ 'first', 'second' ], [ 'second', 'first' ])).toEqual({
      added: [],
      removed: [],
    })
  })
})

describe('getLeadAfter', () => {
  it('keeps the lead while it is still one of the projects', () => {
    expect(getLeadAfter('website', [ 'launch', 'website' ])).toBe('website')
  })

  it('passes the lead to the first project left when the lead leaves', () => {
    expect(getLeadAfter('website', [ 'launch', 'billing' ])).toBe('launch')
  })

  it('gives an item its first project as its lead, and none when it has no project', () => {
    expect(getLeadAfter(null, [ 'launch' ])).toBe('launch')
    expect(getLeadAfter('website', [])).toBeNull()
  })
})

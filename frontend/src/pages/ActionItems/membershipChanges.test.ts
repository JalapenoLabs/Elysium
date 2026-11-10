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

  // Picking a project here would save it as the person's choice, pinning a lead nobody chose.
  it('chooses no lead when the lead leaves, so saving passes it on', () => {
    expect(getLeadAfter('website', [ 'launch', 'billing' ])).toBeNull()
  })

  it('chooses no lead for an item without one, so its first project leads it once saved', () => {
    expect(getLeadAfter(null, [ 'launch' ])).toBeNull()
    expect(getLeadAfter('website', [])).toBeNull()
  })
})

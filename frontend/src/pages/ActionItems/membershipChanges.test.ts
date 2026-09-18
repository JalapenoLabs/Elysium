// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getMembershipChanges } from './membershipChanges'

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

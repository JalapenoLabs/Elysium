// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeActionItem } from '../../testFixtures'
import { pickCurrentItem, skipItem } from './nextRotation'

const ITEMS = [
  makeActionItem({ id: 'first' }),
  makeActionItem({ id: 'second' }),
  makeActionItem({ id: 'third' }),
]

describe('pickCurrentItem', () => {
  it('shows the first item not skipped', () => {
    expect(pickCurrentItem(ITEMS, [])?.id).toBe('first')
    expect(pickCurrentItem(ITEMS, [ 'first' ])?.id).toBe('second')
  })

  it('shows nothing when Next is empty', () => {
    expect(pickCurrentItem([], [ 'first' ])).toBeNull()
  })

  it('shows the next item when the current one is acted on and leaves Next', () => {
    expect(pickCurrentItem(ITEMS.slice(1), [])?.id).toBe('second')
  })
})

describe('skipItem', () => {
  it('sets items aside one after another', () => {
    const afterFirst = skipItem(ITEMS, [], 'first')
    const afterSecond = skipItem(ITEMS, afterFirst, 'second')

    expect(afterSecond).toEqual([ 'first', 'second' ])
    expect(pickCurrentItem(ITEMS, afterSecond)?.id).toBe('third')
  })

  it('comes back around to the top after skipping the last one', () => {
    const skipped = skipItem(ITEMS, [ 'first', 'second' ], 'third')

    expect(skipped).toEqual([ 'third' ])
    expect(pickCurrentItem(ITEMS, skipped)?.id).toBe('first')
  })
})

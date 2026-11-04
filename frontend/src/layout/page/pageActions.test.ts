// Copyright © 2026 Jalapeno Labs

import type { PageAction } from './pageActions'

// Core
import { describe, expect, it } from 'vitest'

// User interface
import { LuTrash2 } from 'react-icons/lu'

// Misc
import { arrangePageActions, isAnyPageActionPending } from './pageActions'

type Flags = {
  isDanger?: boolean
  isPending?: boolean
}

function action(id: string, flags: Flags = {}): PageAction {
  return { id, label: id, icon: LuTrash2, onAction: () => {}, ...flags }
}

describe('arrangePageActions', () => {
  it('lets a lone harmless action stand as its own button', () => {
    const sync = action('sync')
    expect(arrangePageActions([ sync ])).toEqual({ button: sync, regular: [], dangerous: []})
  })

  it('keeps a lone destructive action in the menu, never a bare button', () => {
    const remove = action('delete', { isDanger: true })
    expect(arrangePageActions([ remove ])).toEqual({ button: null, regular: [], dangerous: [ remove ]})
  })

  it('nests more than one action into the menu, destructive ones last, in their order', () => {
    const remove = action('delete', { isDanger: true })
    const sync = action('sync')
    const pause = action('pause')
    expect(arrangePageActions([ remove, sync, pause ])).toEqual({
      button: null,
      regular: [ sync, pause ],
      dangerous: [ remove ],
    })
  })

  it('has nothing to show for no actions', () => {
    expect(arrangePageActions([])).toEqual({ button: null, regular: [], dangerous: []})
  })
})

describe('isAnyPageActionPending', () => {
  it('finds a pending action, inside a submenu too', () => {
    expect(isAnyPageActionPending([ action('sync') ])).toBe(false)
    expect(isAnyPageActionPending([ action('sync', { isPending: true }) ])).toBe(true)
    const snooze: PageAction = {
      id: 'snooze',
      label: 'Snooze',
      icon: LuTrash2,
      items: [ action('tomorrow', { isPending: true }) ],
    }
    expect(isAnyPageActionPending([ action('sync'), snooze ])).toBe(true)
  })
})

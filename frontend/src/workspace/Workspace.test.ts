// Copyright © 2026 Jalapeno Labs

import type { LayoutBase } from 'rc-dock'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { collectTabIds } from './Workspace'

describe('collectTabIds', () => {
  it('collects tabs from nested dock boxes in layout order', () => {
    const layout: LayoutBase = {
      dockbox: {
        mode: 'horizontal',
        children: [
          { tabs: [{ id: 'drive' }, { id: 'git' }]},
          {
            mode: 'vertical',
            children: [
              { tabs: [{ id: 'mail' }]},
            ],
          },
        ],
      },
    }

    expect(collectTabIds(layout)).toEqual([ 'drive', 'git', 'mail' ])
  })

  it('includes floating, popped-out, and maximized regions', () => {
    const layout: LayoutBase = {
      dockbox: {
        mode: 'horizontal',
        children: [{ tabs: [{ id: 'docked' }]}],
      },
      floatbox: {
        mode: 'float',
        children: [{ tabs: [{ id: 'floating' }]}],
      },
      windowbox: {
        mode: 'window',
        children: [{ tabs: [{ id: 'popped' }]}],
      },
      maxbox: {
        mode: 'maximize',
        children: [{ tabs: [{ id: 'maximized' }]}],
      },
    }

    expect(collectTabIds(layout)).toEqual([ 'docked', 'floating', 'popped', 'maximized' ])
  })

  it('skips tabs without ids and tolerates empty panels', () => {
    const layout: LayoutBase = {
      dockbox: {
        mode: 'horizontal',
        children: [
          { tabs: []},
          { tabs: [{}, { id: 'real' }]},
        ],
      },
    }

    expect(collectTabIds(layout)).toEqual([ 'real' ])
  })
})

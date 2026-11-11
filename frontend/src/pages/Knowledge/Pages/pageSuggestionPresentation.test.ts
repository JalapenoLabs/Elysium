// Copyright © 2026 Jalapeno Labs

import type { PageSuggestionChange } from '../../../api/routes/pageRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { acceptLabel, initialChoice, toggleChoice } from './pageSuggestionPresentation'

function change(index: number, applies: boolean): PageSuggestionChange {
  return { index, line: index + 1, before: '- Old', after: '- New', applies }
}

describe('initialChoice', () => {
  it('ticks every change that still merges, and none that the page changed since', () => {
    const chosen = initialChoice([ change(0, true), change(1, false), change(2, true) ])

    expect([ ...chosen ]).toEqual([ 0, 2 ])
  })
})

describe('toggleChoice', () => {
  it('ticks and unticks a change without touching the set it was given', () => {
    const chosen = new Set([ 0 ])

    const ticked = toggleChoice(chosen, 1)
    const unticked = toggleChoice(ticked, 0)

    expect([ ...ticked ].sort()).toEqual([ 0, 1 ])
    expect([ ...unticked ]).toEqual([ 1 ])
    expect([ ...chosen ]).toEqual([ 0 ])
  })
})

describe('acceptLabel', () => {
  it('asks for every change by naming none when all are ticked', () => {
    expect(acceptLabel(new Set([ 1, 0 ]), 2)).toEqual({ kind: 'all', changes: undefined })
  })

  it('names the changes taken, in order, when only some are', () => {
    expect(acceptLabel(new Set([ 2, 0 ]), 3)).toEqual({ kind: 'some', changes: [ 0, 2 ], taken: 2, count: 3 })
  })
})

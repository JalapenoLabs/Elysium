// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { swapsWhatPageEdits } from './pageSave'

describe('swapsWhatPageEdits', () => {
  it('never asks about the passage a knowledge page opened at', () => {
    expect(swapsWhatPageEdits('?passage=2', '?passage=3')).toBe(false)
  })

  it('asks when the query can pick another record', () => {
    expect(swapsWhatPageEdits('?project=one', '?project=two')).toBe(true)
  })

  it('never asks about the open tab', () => {
    expect(swapsWhatPageEdits('', '?tab=history')).toBe(false)
  })

  it('never asks about what a knowledge page\'s history shows', () => {
    expect(swapsWhatPageEdits('?tab=history', '?tab=history&version=3')).toBe(false)
    expect(swapsWhatPageEdits('?tab=history&version=3', '?tab=history&from=2&to=3')).toBe(false)
  })

  it('ignores the order a query is written in', () => {
    expect(swapsWhatPageEdits('?a=1&b=2', '?b=2&a=1')).toBe(false)
  })
})

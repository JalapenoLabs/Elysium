// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'

// Misc
import { DateText } from './DateText'

// Noon UTC, so the day is the same in every zone a test machine might run in.
const MIDSUMMER = '2026-06-15T12:00:00Z'

describe('DateText', () => {
  beforeEach(() => {
    vi.useFakeTimers({ now: Date.parse('2026-09-01T12:00:00Z'), toFake: [ 'Date' ]})
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows the short form and carries the instant for machines', () => {
    render(<DateText value={MIDSUMMER} />)

    const time = screen.getByText('June 15th')
    expect(time.tagName).toBe('TIME')
    expect(time.getAttribute('datetime')).toBe('2026-06-15T12:00:00.000Z')
  })

  it('offers the full timestamp from a focusable trigger', () => {
    render(<DateText value={MIDSUMMER} />)

    const trigger = screen.getByText('June 15th').closest('[data-slot="tooltip-trigger"]')
    expect(trigger?.getAttribute('tabindex')).toBe('0')
    expect(trigger?.getAttribute('role')).toBe('button')
  })

  it('leaves the tab order, and stops calling itself a button, when it is not focusable', () => {
    render(<DateText value={MIDSUMMER} isFocusable={false} />)

    const trigger = screen.getByText('June 15th').closest('[data-slot="tooltip-trigger"]')
    expect(trigger?.getAttribute('tabindex')).toBe('-1')
    expect(trigger?.getAttribute('role')).toBe('presentation')
  })
})

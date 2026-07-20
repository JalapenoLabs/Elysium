// Copyright © 2026 Jalapeno Labs

// Core
import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'

// Misc
import { OhLookAtTheTime } from './OhLookAtTheTime'
import { Timestamp } from '../proto/helpers/timestamp'

// A fixed instant, pinned to UTC so the formatted output is deterministic.
const instant = Timestamp.fromDate(new Date('2026-07-07T15:30:00Z'), 'Etc/UTC').proto

afterEach(() => {
  cleanup()
})

describe('OhLookAtTheTime', () => {
  it('renders the compact absolute instant by default', () => {
    render(<OhLookAtTheTime timestamp={instant} timezone='Etc/UTC' />)
    expect(screen.getByText('Jul 7, 3:30 PM')).toBeTruthy()
  })

  it('exposes the machine-readable instant on the <time> element', () => {
    render(<OhLookAtTheTime timestamp={instant} timezone='Etc/UTC' />)
    const element = screen.getByText('Jul 7, 3:30 PM')
    expect(element.tagName).toBe('TIME')
    expect(element.getAttribute('dateTime')).toBe('2026-07-07T15:30:00.000Z')
  })

  it('renders the instant in the given display zone, not UTC', () => {
    // 15:30 UTC is 11:30 in New York during July (EDT, UTC-4).
    render(<OhLookAtTheTime timestamp={instant} timezone='America/New_York' />)
    expect(screen.getByText('Jul 7, 11:30 AM')).toBeTruthy()
  })

  it('renders a relative instant when asked', () => {
    render(<OhLookAtTheTime timestamp={instant} relative />)
    // A long-past instant always reads as "... ago"; the exact span drifts with
    // wall-clock time, so assert only the relative suffix.
    expect(screen.getByText(/ago$/)).toBeTruthy()
  })
})

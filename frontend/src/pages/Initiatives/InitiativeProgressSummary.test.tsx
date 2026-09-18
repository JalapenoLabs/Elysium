// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'

// User interface
import { InitiativeProgressSummary } from './InitiativeProgressSummary'

describe('InitiativeProgressSummary', () => {
  it('shows resolved and total together, never a percentage', () => {
    const { container } = render(<InitiativeProgressSummary progress={{ resolved: 3, total: 8 }} />)

    expect(screen.getByText('3 of 8 resolved')).toBeTruthy()
    expect(container.textContent).not.toContain('%')
  })

  it('draws an empty initiative without dividing by zero', () => {
    const { container } = render(<InitiativeProgressSummary progress={{ resolved: 0, total: 0 }} />)

    expect(screen.getByText('0 of 0 resolved')).toBeTruthy()
    expect(container.querySelector<HTMLElement>('[style]')?.style.width).toBe('0%')
  })
})

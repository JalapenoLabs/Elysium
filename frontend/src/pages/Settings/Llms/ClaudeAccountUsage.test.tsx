// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'

// User interface
import { ClaudeAccountUsage } from './ClaudeAccountUsage'

// Misc
import { makeClaudeAccount } from '../../../testFixtures'

describe('ClaudeAccountUsage', () => {
  it('shows each window Anthropic reported, with when it resets', () => {
    render(<ClaudeAccountUsage account={makeClaudeAccount({
      usage: {
        fiveHour: { utilization: 64.4, resetsAt: '2026-10-21T05:00:00Z' },
        sevenDay: { utilization: 12, resetsAt: null },
        readAt: '2026-10-21T02:00:00Z',
      },
    })} />)

    const meters = screen.getAllByRole('meter')
    expect(meters).toHaveLength(2)
    expect(meters[0].getAttribute('aria-valuenow')).toBe('64')
    expect(screen.getByText('Five hours')).toBeTruthy()
    expect(screen.getByText('Seven days')).toBeTruthy()
    expect(screen.getAllByText(/^Resets /)).toHaveLength(1)
  })

  it('caps a window reported past its limit at a full bar', () => {
    render(<ClaudeAccountUsage account={makeClaudeAccount({
      usage: {
        fiveHour: { utilization: 130, resetsAt: null },
        sevenDay: null,
        readAt: '2026-10-21T02:00:00Z',
      },
    })} />)

    expect(screen.getByRole('meter').getAttribute('aria-valuenow')).toBe('100')
  })

  it('says when usage has not been read, or could not be', () => {
    const { unmount } = render(<ClaudeAccountUsage account={makeClaudeAccount()} />)
    expect(screen.getByText('Not read yet')).toBeTruthy()
    unmount()

    render(<ClaudeAccountUsage account={makeClaudeAccount({ usageError: 'Anthropic answered 500' })} />)
    expect(screen.getByText('Could not be read')).toBeTruthy()
  })
})

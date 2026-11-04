// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'

// User interface
import { ClaudeUsageWindow } from './ClaudeUsageWindow'

// Misc
import { makeClaudeAccount } from '../../../testFixtures'

const READ_USAGE = {
  fiveHour: { utilization: 64.4, resetsAt: '2026-10-21T05:00:00Z' },
  sevenDay: { utilization: 12, resetsAt: null },
  readAt: '2026-10-21T02:00:00Z',
}

describe('ClaudeUsageWindow', () => {
  it('shows the window it is given, named for screen readers, with when it resets', () => {
    render(<ClaudeUsageWindow
      account={makeClaudeAccount({ usage: READ_USAGE })}
      window='fiveHour'
      label='Five-hour usage'
    />)

    const meter = screen.getByRole('meter', { name: 'Five-hour usage' })
    expect(meter.getAttribute('aria-valuenow')).toBe('64')
    expect(screen.getAllByText(/^Resets /)).toHaveLength(1)
  })

  it('leaves the reset out when Anthropic did not say', () => {
    render(<ClaudeUsageWindow
      account={makeClaudeAccount({ usage: READ_USAGE })}
      window='sevenDay'
      label='Seven-day usage'
    />)

    expect(screen.getByRole('meter').getAttribute('aria-valuenow')).toBe('12')
    expect(screen.queryByText(/^Resets /)).toBeNull()
  })

  it('caps a window reported past its limit at a full bar', () => {
    render(<ClaudeUsageWindow
      account={makeClaudeAccount({
        usage: {
          fiveHour: { utilization: 130, resetsAt: null },
          sevenDay: null,
          readAt: '2026-10-21T02:00:00Z',
        },
      })}
      window='fiveHour'
      label='Five-hour usage'
    />)

    expect(screen.getByRole('meter').getAttribute('aria-valuenow')).toBe('100')
  })

  it('says when usage has not been read, could not be, or has no such window', () => {
    const { unmount } = render(<ClaudeUsageWindow
      account={makeClaudeAccount()}
      window='fiveHour'
      label='Five-hour usage'
    />)
    expect(screen.getByText('Not read yet')).toBeTruthy()
    unmount()

    const failed = render(<ClaudeUsageWindow
      account={makeClaudeAccount({ usageError: 'Anthropic answered 500' })}
      window='fiveHour'
      label='Five-hour usage'
    />)
    expect(screen.getByText('Could not be read')).toBeTruthy()
    failed.unmount()

    render(<ClaudeUsageWindow
      account={makeClaudeAccount({ usage: { ...READ_USAGE, sevenDay: null }})}
      window='sevenDay'
      label='Seven-day usage'
    />)
    expect(screen.getByText('Not available')).toBeTruthy()
  })

  it('says a credential that is not a signed-in account reports none', () => {
    render(<ClaudeUsageWindow account={null} window='fiveHour' label='Five-hour usage' />)

    expect(screen.getByText('Not available')).toBeTruthy()
  })
})

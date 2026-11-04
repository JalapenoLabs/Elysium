// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'

// User interface
import { MeetingSources } from './MeetingSources'

// Misc
import { makeConnector } from '../../../testFixtures'

describe('MeetingSources', () => {
  it('with no source connected, names every kind that provides meetings and links to adding it', () => {
    render(<MemoryRouter>
      <MeetingSources kinds={[ 'fireflies' ]} connectors={[]} />
    </MemoryRouter>)

    expect(screen.getByText('No meeting source yet. Connect one to read your meetings:')).toBeTruthy()
    const add = screen.getByRole('link', { name: 'Fireflies' })
    expect(add.getAttribute('href')).toBe('/settings/connectors/new/fireflies')
  })

  it('lists each connected source, linking to its connector rather than offering to add one', () => {
    render(<MemoryRouter>
      <MeetingSources
        kinds={[ 'fireflies' ]}
        connectors={[ makeConnector({
          id: 'connector-1',
          kind: 'fireflies',
          status: 'error',
          account: { id: 'key-1', kind: 'fireflies', name: 'Work notes' },
        }) ]}
      />
    </MemoryRouter>)

    const source = screen.getByRole('link', { name: 'Fireflies' })
    expect(source.getAttribute('href')).toBe('/settings/connectors/connector-1')
    expect(screen.getByText('Work notes')).toBeTruthy()
    expect(screen.queryByText('No meeting source yet. Connect one to read your meetings:')).toBeNull()
  })
})

// Copyright © 2026 Jalapeno Labs

// Core
import { render, screen } from '@testing-library/react'
import { createMemoryRouter, RouterProvider, useLocation } from 'react-router'
import { describe, expect, it } from 'vitest'

// User interface
import { MeetingRedirect } from './MeetingRedirect'

// Misc
import { UrlTree } from '../../../urls'

function Landed() {
  const location = useLocation()
  return <p>{`${location.pathname}${location.search}`}</p>
}

describe('MeetingRedirect', () => {
  it('sends a meeting\'s Settings address to its page in the knowledge base, keeping its tab', () => {
    const router = createMemoryRouter(
      [
        { path: UrlTree.settingsMeeting, element: <MeetingRedirect /> },
        { path: UrlTree.knowledgeMeeting, element: <Landed /> },
      ],
      { initialEntries: [ '/settings/meetings/meeting-1?tab=topics' ]},
    )

    render(<RouterProvider router={router} />)

    expect(screen.getByText('/knowledge/meetings/meeting-1?tab=topics')).toBeDefined()
    expect(router.state.historyAction).toBe('REPLACE')
  })
})

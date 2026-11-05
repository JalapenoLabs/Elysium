// Copyright © 2026 Jalapeno Labs

// Core
import { render, screen } from '@testing-library/react'
import { createMemoryRouter, RouterProvider, useLocation } from 'react-router'
import { describe, expect, it } from 'vitest'

// User interface
import { LearningRedirect } from './LearningRedirect'

// Misc
import { UrlTree } from '../../../urls'

function Landed() {
  return <p>{useLocation().pathname}</p>
}

describe('LearningRedirect', () => {
  it('sends a learning\'s old address to its page, which has the same id', () => {
    const router = createMemoryRouter(
      [
        { path: UrlTree.knowledgeLearning, element: <LearningRedirect /> },
        { path: UrlTree.knowledgePage, element: <Landed /> },
      ],
      { initialEntries: [ '/knowledge/learnings/learning-1' ]},
    )

    render(<RouterProvider router={router} />)

    expect(screen.getByText('/knowledge/pages/learning-1')).toBeDefined()
    expect(router.state.historyAction).toBe('REPLACE')
  })
})

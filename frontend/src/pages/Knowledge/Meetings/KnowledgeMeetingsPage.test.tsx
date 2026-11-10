// Copyright © 2026 Jalapeno Labs

// Core
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider, useLocation } from 'react-router'
import { describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'
import { meetingsLoaded } from '../../../store/meetingsSlice'
import { projectsLoaded } from '../../../store/projectsSlice'

// User interface
import { KnowledgePageView } from '../Pages/KnowledgePageView'
import { MeetingsPage } from '../../Settings/Meetings/MeetingsPage'
import { KnowledgeMeetingsPage } from './KnowledgeMeetingsPage'

// Misc
import { MEETINGS_PAGE_ID } from '../../../constants'
import { makeMeeting, makeProject } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'
import { getKnowledgePageUrl, getProjectMeetingsUrl, UrlTree } from '../../../urls'

// What the page reads is in the store already; the loaders are the network boundary.
vi.mock('../../../hooks/useServerData', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../hooks/useServerData')>(),
  useMeetingsLoader: () => 'loaded',
  useProjectsLoader: () => 'loaded',
  useConnectorsLoader: () => 'loaded',
}))

vi.mock('../../Settings/Connectors/useConnectorKinds', () => ({
  useConnectorKinds: () => ({ kinds: [], isLoading: false, error: null }),
}))

function Landed() {
  const location = useLocation()
  return <p>{`Landed on ${location.pathname}${location.search}`}</p>
}

function renderAt(address: string) {
  const store = createAppStore()
  store.dispatch(projectsLoaded([ makeProject({ id: 'launch', name: 'Launch' }) ]))
  store.dispatch(meetingsLoaded([
    makeMeeting({ id: 'standup', title: 'Standup', projectIds: [ 'launch' ], awaitingApproval: true }),
    makeMeeting({ id: 'one-to-one', title: 'One to one' }),
  ]))
  const router = createMemoryRouter(
    [
      {
        path: UrlTree.knowledgeMeetings,
        element: <TestAriaRouter>
          <KnowledgeMeetingsPage />
        </TestAriaRouter>,
      },
      { path: UrlTree.knowledgePage, element: <KnowledgePageView /> },
      { path: UrlTree.settingsMeetings, element: <MeetingsPage /> },
      { path: '*', element: <Landed /> },
    ],
    { initialEntries: [ address ]},
  )
  render(<Provider store={store}>
    <RouterProvider router={router} />
  </Provider>)
  return router
}

describe('KnowledgeMeetingsPage', () => {
  it('lists every meeting, those on no project included, and counts what waits for review', () => {
    renderAt(UrlTree.knowledgeMeetings)

    expect(screen.getByRole('heading', { name: 'Meetings' })).toBeDefined()
    expect(screen.getByText('Standup')).toBeDefined()
    expect(screen.getByText('One to one')).toBeDefined()
    expect(screen.getByText('1 waiting for review')).toBeDefined()
    const sources = screen.getByRole('link', { name: 'Meeting sources' })
    expect(sources.getAttribute('href')).toBe(UrlTree.settingsMeetings)
  })

  it('lists a project\'s meetings alone, as its Meetings group in the tree opens it', () => {
    renderAt(getProjectMeetingsUrl('launch'))

    expect(screen.getByText('Standup')).toBeDefined()
    expect(screen.queryByText('One to one')).toBeNull()
  })

  it('says when the filters leave no meeting', () => {
    renderAt(`${UrlTree.knowledgeMeetings}?state=failed`)

    expect(screen.getByText('No meeting matches these filters.')).toBeDefined()
  })
})

describe('the Meetings page\'s other addresses', () => {
  it('opens the Meetings page\'s address as a page on the Meetings list', async () => {
    const router = renderAt(getKnowledgePageUrl(MEETINGS_PAGE_ID))

    expect(await screen.findByRole('heading', { name: 'Meetings' })).toBeDefined()
    expect(router.state.location.pathname).toBe(UrlTree.knowledgeMeetings)
    expect(router.state.historyAction).toBe('REPLACE')
  })

  it('sends Settings, Meetings\' old meetings tab to the Meetings list', async () => {
    const router = renderAt(`${UrlTree.settingsMeetings}?tab=meetings`)

    expect(await screen.findByRole('heading', { name: 'Meetings' })).toBeDefined()
    expect(router.state.location.pathname).toBe(UrlTree.knowledgeMeetings)
  })
})

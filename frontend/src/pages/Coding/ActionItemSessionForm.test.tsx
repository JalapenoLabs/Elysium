// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../../store'
import { actionItemUpserted } from '../../store/actionItemsSlice'

// User interface
import { ActionItemSessionForm } from './ActionItemSessionForm'

// Misc
import { makeActionItem } from '../../testFixtures'

// What each loader answers; the requests themselves are the network boundary.
const loaderStatus = vi.hoisted(() => ({
  actionItem: 'loaded',
  projects: 'loaded',
  satellites: 'loaded',
}))

vi.mock('../../hooks/useServerData', () => ({
  useActionItemLoader: () => loaderStatus.actionItem,
  useProjectsLoader: () => loaderStatus.projects,
  useSatellitesLoader: () => loaderStatus.satellites,
}))

// The form itself presets from what is loaded when it mounts; this stands in for it so the
// test sees exactly when it mounts, and with which item.
vi.mock('./CreateSessionForm', () => ({
  CreateSessionForm: (props: { actionItem: ActionItem }) => <p>{`Form for ${props.actionItem.title}`}</p>,
}))

function renderFor(actionItemId: string, items: ActionItem[]) {
  const store = createAppStore()
  for (const item of items) {
    store.dispatch(actionItemUpserted(item))
  }
  return render(<Provider store={store}>
    <MemoryRouter>
      <ActionItemSessionForm actionItemId={actionItemId} onCreated={() => {}} />
    </MemoryRouter>
  </Provider>)
}

describe('ActionItemSessionForm', () => {
  beforeEach(() => {
    loaderStatus.actionItem = 'loaded'
    loaderStatus.projects = 'loaded'
    loaderStatus.satellites = 'loaded'
  })

  it('waits for the projects before mounting the form, which presets its project from them', () => {
    loaderStatus.projects = 'loading'
    const item = makeActionItem({ id: 'login', title: 'Fix the login bug', projectIds: [ 'elysium' ]})
    const loading = renderFor('login', [ item ])
    expect(screen.queryByText('Form for Fix the login bug')).toBeNull()
    loading.unmount()

    loaderStatus.projects = 'loaded'
    renderFor('login', [ item ])
    expect(screen.getByText('Form for Fix the login bug')).toBeTruthy()
  })

  it('waits for the satellites too', () => {
    loaderStatus.satellites = 'loading'
    renderFor('login', [ makeActionItem({ id: 'login', title: 'Fix the login bug' }) ])

    expect(screen.queryByText('Form for Fix the login bug')).toBeNull()
  })

  it('says so instead of mounting a form it could never submit when the projects fail to load', () => {
    loaderStatus.projects = 'failed'
    renderFor('login', [ makeActionItem({ id: 'login', title: 'Fix the login bug', projectIds: [ 'elysium' ]}) ])

    expect(screen.queryByText('Form for Fix the login bug')).toBeNull()
    expect(screen.getByText('Projects or satellites could not be loaded. Close this and try again.')).toBeTruthy()
  })

  it('starts nothing from a deleted item', () => {
    renderFor('login', [ makeActionItem({ id: 'login', deletedAt: '2026-09-10T00:00:00.000Z' }) ])

    expect(screen.getByText('This action item is deleted. Restore it before starting a session from it.')).toBeTruthy()
  })

  it('says so when the item cannot be found', () => {
    loaderStatus.actionItem = 'failed'
    renderFor('missing', [])

    expect(screen.getByText('This action item could not be found.')).toBeTruthy()
  })
})

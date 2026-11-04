// Copyright © 2026 Jalapeno Labs

import type { ProjectManager, ProjectManagerTurn } from '../../api/routes/projectManagerRoutes'

// Core
import { describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../store'
import { projectManagerLoaded, turnLogLoaded } from '../../store/projectManagersSlice'

// User interface
import { ProjectManagerPanel } from './ProjectManagerPanel'

// Misc
import { ELYSIA_USER_ID } from '../../constants'

// Everything shown is read from the store; nothing is fetched.
vi.mock('../../hooks/useServerData', () => ({
  useProjectManagerTurnLoader: () => 'loaded',
  useUsersLoader: () => 'loaded',
  useJiraCredentialsLoader: () => 'loaded',
  useConnectorsLoader: () => 'loaded',
}))

// Proposing, with its issues filing where the project's issues already are.
const PROPOSING: ProjectManager['writes'] = {
  direct: false,
  tracker: {
    source: 'precedent',
    provider: 'jira',
    credentialId: 'work-jira',
    credentialName: 'Work',
    destination: 'ELY',
  },
  trackerRefusal: null,
}

function turn(overrides: Partial<ProjectManagerTurn> & Pick<ProjectManagerTurn, 'id'>): ProjectManagerTurn {
  return {
    status: 'succeeded',
    summary: null,
    reply: null,
    error: null,
    timelineRecordId: null,
    startedAt: '2026-11-04T15:00:00Z',
    finishedAt: '2026-11-04T15:02:00Z',
    wakes: [],
    ...overrides,
  }
}

function renderPanel(manager: ProjectManager) {
  const store = createAppStore()
  store.dispatch(projectManagerLoaded(manager))
  render(<Provider store={store}>
    <ProjectManagerPanel projectId={manager.projectId} status='loaded' />
  </Provider>)
  return store
}

const planned = turn({
  id: 'turn-2',
  summary: 'Submitted the annual pricing batch.',
  reply: 'A worker will put annual pricing on the page today.',
  wakes: [
    {
      id: 'wake-1',
      kind: 'message',
      timelineRecordId: null,
      workBatchId: null,
      promiseId: null,
      body: 'Leadership wants annual pricing live this week.',
      createdBy: ELYSIA_USER_ID,
      createdAt: '2026-11-04T14:59:00Z',
    },
    {
      id: 'wake-2',
      kind: 'record',
      timelineRecordId: 'record-1',
      workBatchId: null,
      promiseId: null,
      body: null,
      createdBy: ELYSIA_USER_ID,
      createdAt: '2026-11-04T14:58:00Z',
    },
  ],
})

const failed = turn({
  id: 'turn-1',
  status: 'failed',
  error: 'Every Claude credential reached its usage limit.',
  startedAt: '2026-11-04T14:00:00Z',
})

describe('ProjectManagerPanel', () => {
  it('shows what it is doing and collapses each turn to its summary until it is opened', () => {
    const store = renderPanel({
      projectId: 'launch',
      status: 'waiting',
      waiting: 3,
      usagePausedUntil: null,
      turns: [ planned, failed ],
      batches: [],
      promises: [],
      writes: PROPOSING,
    })

    expect(screen.getByText('Starting soon')).toBeTruthy()
    expect(screen.getByText('3 changes wait for its next turn')).toBeTruthy()
    expect(screen.getByText('Submitted the annual pricing batch.')).toBeTruthy()
    expect(screen.queryByText('Leadership wants annual pricing live this week.')).toBeNull()

    // A failed turn starts open, saying why.
    expect(screen.getAllByText('Every Claude credential reached its usage limit.').length).toBeGreaterThan(0)

    store.dispatch(turnLogLoaded({ turnId: 'turn-2', log: 'Called manager_submit_batch\n' }))
    fireEvent.click(screen.getByText('Submitted the annual pricing batch.'))
    expect(screen.getByText('Leadership wants annual pricing live this week.')).toBeTruthy()
    expect(screen.getByText('Elysia')).toBeTruthy()
    expect(screen.getByText('1 record new on the timeline')).toBeTruthy()
    expect(screen.getByText('A worker will put annual pricing on the page today.')).toBeTruthy()
    expect(screen.getByText('Called manager_submit_batch')).toBeTruthy()
  })

  it('says when nothing has woken it yet', () => {
    renderPanel({
      projectId: 'launch',
      status: 'idle',
      waiting: 0,
      usagePausedUntil: null,
      turns: [],
      batches: [],
      promises: [],
      writes: PROPOSING,
    })
    expect(screen.getByText('Nothing has woken the Project Manager yet.')).toBeTruthy()
  })

  it('says whether its changes wait for review and where its issues file, or why nowhere', () => {
    const idle: ProjectManager = {
      projectId: 'launch',
      status: 'idle',
      waiting: 0,
      usagePausedUntil: null,
      turns: [],
      batches: [],
      promises: [],
      writes: PROPOSING,
    }
    const store = renderPanel(idle)
    const writesDirectly = screen.getByRole('switch', { name: 'Write to trackers without review' })
    expect(writesDirectly).toHaveProperty('checked', false)
    expect(screen.getByText('Files issues in Jira ELY through Work')).toBeTruthy()
    expect(screen.getByText('Where this project\'s issues already are')).toBeTruthy()

    act(() => {
      store.dispatch(projectManagerLoaded({
        ...idle,
        writes: { direct: true, tracker: null, trackerRefusal: 'this project names no tracker' },
      }))
    })
    expect(screen.getByRole('switch', { name: 'Write to trackers without review' })).toHaveProperty('checked', true)
    expect(screen.getByText('this project names no tracker')).toBeTruthy()
  })
})

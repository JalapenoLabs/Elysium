// Copyright © 2026 Jalapeno Labs

import type { TimelineRecord } from '../../api/routes/timelineRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'

// Redux
import { createAppStore } from '../../store'
import { projectsLoaded } from '../../store/projectsSlice'
import { timelineRecordsLoaded } from '../../store/timelineRecordsSlice'

// User interface
import { RecordProjects } from './RecordProjects'

// Misc
import { makeProject, makeTimelineRecord } from '../../testFixtures'

// Project names are read from the store; nothing is fetched.
vi.mock('../../hooks/useServerData', () => ({
  useProjectsLoader: () => 'loaded',
}))

// The attribution routes are the network boundary; toasts are watched for what they say and the
// Undo they offer.
const routes = vi.hoisted(() => ({
  attributeTimelineRecord: vi.fn(),
  unattributeTimelineRecord: vi.fn(),
  undoRecordChange: vi.fn(),
}))

vi.mock('../../api/routes/timelineRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/timelineRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

// A meeting topic Elysia put on the launch, 72% sure.
function guessed(): TimelineRecord {
  return makeTimelineRecord({
    id: 'record-1',
    projects: [{ projectId: 'launch', confidence: 0.72, attributedBy: 'elysia', createdAt: '2026-11-04T12:00:00Z' }],
  })
}

function renderProjects(record: TimelineRecord) {
  const store = createAppStore()
  store.dispatch(projectsLoaded([
    makeProject({ id: 'launch', name: 'Launch' }),
    makeProject({ id: 'website', name: 'Website' }),
  ]))
  store.dispatch(timelineRecordsLoaded([ record ]))
  render(<Provider store={store}>
    <RecordProjects record={record} />
  </Provider>)
  return store
}

describe('RecordProjects', () => {
  beforeEach(() => {
    for (const route of Object.values(routes)) {
      route.mockReset()
    }
    for (const shown of Object.values(toasts)) {
      shown.mockReset()
    }
  })

  it('says how sure Elysia was, and offers every project the record is not on', () => {
    renderProjects(guessed())

    expect(screen.getByText('Launch')).toBeTruthy()
    expect(screen.getByText('Elysia, 72% sure')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Confirm it is about Launch' })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Add to a project/ })).toBeTruthy()
  })

  it('takes a record off a project at once, with Undo that puts it back', async () => {
    const record = guessed()
    routes.unattributeTimelineRecord.mockResolvedValue({ record: { ...record, projects: []}, entryId: 'entry-2' })
    routes.undoRecordChange.mockResolvedValue({ record, entryId: 'entry-3' })
    const store = renderProjects(record)

    fireEvent.click(screen.getByRole('button', { name: 'Take off Launch' }))

    await waitFor(() => expect(store.getState().timelineRecords.entities['record-1']?.projects).toEqual([]))
    expect(routes.unattributeTimelineRecord).toHaveBeenCalledWith('record-1', 'launch')
    const [ message, options ] = toasts.success.mock.calls[0]
    expect(message).toBe('Taken off Launch')
    expect(options.action.label).toBe('Undo')

    options.action.onPress()
    await waitFor(() => expect(routes.undoRecordChange).toHaveBeenCalledWith('record-1', 'entry-2'))
    await waitFor(() => expect(store.getState().timelineRecords.entities['record-1']?.projects).toHaveLength(1))
    expect(toasts.success).toHaveBeenLastCalledWith('Change undone')
  })

  it('says so, rather than failing, when what it would undo changed again since', async () => {
    const record = guessed()
    routes.attributeTimelineRecord.mockResolvedValue({ record, entryId: 'entry-2' })
    // The API's 409 as ky hands it on: an HTTPError carrying the body it read.
    const { HTTPError } = await import('ky')
    const changedSince: unknown = Object.assign(Object.create(HTTPError.prototype), {
      data: { code: 'attribution_changed', message: 'changed since' },
    })
    routes.undoRecordChange.mockRejectedValue(changedSince)
    renderProjects(record)

    fireEvent.click(screen.getByRole('button', { name: 'Confirm it is about Launch' }))
    await waitFor(() => expect(toasts.success).toHaveBeenCalled())
    expect(toasts.success.mock.calls[0][0]).toBe('Kept on Launch')

    toasts.success.mock.calls[0][1].action.onPress()
    await waitFor(() => expect(toasts.warning).toHaveBeenCalledWith(
      'This changed again since, so the newer change stays.',
    ))
    expect(toasts.danger).not.toHaveBeenCalled()
  })
})

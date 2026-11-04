// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  runLoaded,
  runsPageLoaded,
  selectRunById,
  selectRunHistory,
  selectRuns,
  selectStepLog,
  selectStepsOfRun,
  stepLogLoaded,
} from './runsSlice'

// Misc
import { makeRun, makeRunStep } from '../testFixtures'

// UUIDv7-like ids, which sort the way runs were started.
const OLDER = '01920000-0000-7000-8000-000000000001'
const NEWER = '01920000-0000-7000-8000-000000000002'

describe('runsSlice', () => {
  it('merges pages newest first and remembers the list was read', () => {
    const store = createAppStore()
    expect(store.getState().runs.isListLoaded).toBe(false)

    store.dispatch(runsPageLoaded([ makeRun({ id: NEWER }) ]))
    store.dispatch(runsPageLoaded([ makeRun({ id: OLDER }) ]))

    const state = store.getState()
    expect(selectRuns(state).map((run) => run.id)).toEqual([ NEWER, OLDER ])
    expect(state.runs.isListLoaded).toBe(true)
  })

  it('replaces a run, its steps in order, and its history whole when its page loads it', () => {
    const store = createAppStore()
    const first = makeRunStep({ id: 'step-1', runId: NEWER, state: 'running' })
    const second = makeRunStep({ id: 'step-2', runId: NEWER, step: 'segment' })
    store.dispatch(runLoaded({ run: makeRun({ id: NEWER }), steps: [ first, second ], history: []}))

    store.dispatch(runLoaded({
      run: makeRun({ id: NEWER, state: 'failed', error: 'no transcript' }),
      steps: [{ ...first, state: 'failed' }, second ],
      history: [{
        id: 'entry',
        runId: NEWER,
        stepJobId: null,
        kind: 'started',
        createdBy: 'person',
        data: {},
        createdAt: '2026-11-03T00:00:00Z',
      }],
    }))

    const state = store.getState()
    expect(selectRunById(state, NEWER)?.state).toBe('failed')
    expect(selectStepsOfRun(state, NEWER).map((step) => [ step.id, step.state ])).toEqual([
      [ 'step-1', 'failed' ],
      [ 'step-2', 'pending' ],
    ])
    expect(selectRunHistory(state, NEWER).map((entry) => entry.kind)).toEqual([ 'started' ])
    expect(selectStepsOfRun(state, OLDER)).toEqual([])
  })

  it('keeps a step log only once a page opened it', () => {
    const store = createAppStore()
    expect(selectStepLog(store.getState(), 'step-1')).toBeUndefined()

    store.dispatch(stepLogLoaded({ stepId: 'step-1', log: '' }))
    expect(selectStepLog(store.getState(), 'step-1')).toBe('')

    store.dispatch(stepLogLoaded({ stepId: 'step-1', log: 'fetching\n' }))
    expect(selectStepLog(store.getState(), 'step-1')).toBe('fetching\n')
  })
})

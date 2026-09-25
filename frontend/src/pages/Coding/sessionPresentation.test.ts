// Copyright © 2026 Jalapeno Labs

import type { ThreadStatus } from '../../api/routes/codingSessionRoutes'
import type { SessionContext, SessionTranslate } from './sessionPresentation'

// Core
import { beforeEach, describe, expect, it } from 'vitest'
import i18next from 'i18next'

// Misc
import { makeCodingSession } from '../../testFixtures'
import { describeSession, searchSessionSummaries } from './sessionPresentation'

type Context = {
  t: SessionTranslate
  context: SessionContext
}

function makeThread(overrides: Partial<ThreadStatus>): ThreadStatus {
  return {
    state: 'idle',
    queueDepth: 0,
    currentTurnId: null,
    latestSequence: 0,
    lastActivityAt: null,
    expiresAt: null,
    ...overrides,
  }
}

describe('describeSession', () => {
  beforeEach<Context>((testContext) => {
    // The app's own en-US strings, loaded by the test setup.
    testContext.t = i18next.getFixedT(null, 'coding')
    testContext.context = {
      projectNames: { elysium: 'Elysium' },
      satelliteNames: { forge: 'Forge' },
      formatInstant: (instant) => `<${instant}>`,
    }
  })

  it<Context>('names the project, satellite, state, and last activity', ({ t, context }) => {
    const session = makeCodingSession({
      id: 4,
      projectId: 'elysium',
      satelliteId: 'forge',
      thread: makeThread({ state: 'awaiting-input', lastActivityAt: '2026-09-20T12:00:00.000Z' }),
    })

    expect(describeSession(session, t, context)).toEqual({
      session,
      projectName: 'Elysium',
      satelliteName: 'Forge',
      state: 'awaiting-input',
      stateLabel: 'Needs input',
      lastActivity: '<2026-09-20T12:00:00.000Z>',
    })
  })

  it<Context>('reads a session the API has not polled yet as unknown with no activity', ({ t, context }) => {
    const summary = describeSession(makeCodingSession({ id: 1, projectId: 'elysium' }), t, context)

    expect(summary.state).toBe('unknown')
    expect(summary.stateLabel).toBe('Unknown')
    expect(summary.lastActivity).toBe('No activity yet')
  })

  it<Context>('leaves the name blank for a project or satellite it does not know', ({ t, context }) => {
    const summary = describeSession(makeCodingSession({ id: 1, projectId: 'gone', satelliteId: 'gone' }), t, context)

    expect(summary.projectName).toBe('')
    expect(summary.satelliteName).toBe('')
  })
})

describe('searchSessionSummaries', () => {
  beforeEach<Context>((testContext) => {
    testContext.t = i18next.getFixedT(null, 'coding')
    testContext.context = {
      projectNames: { elysium: 'Elysium', farworlds: 'Farworlds' },
      satelliteNames: { forge: 'Forge', anvil: 'Anvil' },
      formatInstant: (instant) => instant,
    }
  })

  function describeAll({ t, context }: Context) {
    return [
      makeCodingSession({
        id: 12,
        title: 'Fix the login page',
        projectId: 'elysium',
        satelliteId: 'forge',
        thread: makeThread({ state: 'running', lastActivityAt: '2026-09-20T09:00:00.000Z' }),
      }),
      makeCodingSession({
        id: 3,
        title: 'Write the storage docs',
        projectId: 'farworlds',
        satelliteId: 'anvil',
      }),
    ].map((session) => describeSession(session, t, context))
  }

  function ids(summaries: { session: { id: number } }[]) {
    return summaries.map((summary) => summary.session.id)
  }

  it<Context>('lists every session, in order, when the search is blank', (testContext) => {
    const summaries = describeAll(testContext)

    expect(ids(searchSessionSummaries(summaries, ''))).toEqual([ 12, 3 ])
    expect(ids(searchSessionSummaries(summaries, '   '))).toEqual([ 12, 3 ])
  })

  it<Context>('matches the title, project, satellite, and state as shown, ignoring case', (testContext) => {
    const summaries = describeAll(testContext)

    expect(ids(searchSessionSummaries(summaries, 'LOGIN'))).toEqual([ 12 ])
    expect(ids(searchSessionSummaries(summaries, 'farworlds'))).toEqual([ 3 ])
    expect(ids(searchSessionSummaries(summaries, 'anvil'))).toEqual([ 3 ])
    expect(ids(searchSessionSummaries(summaries, 'running'))).toEqual([ 12 ])
    expect(ids(searchSessionSummaries(summaries, 'no activity'))).toEqual([ 3 ])
  })

  it<Context>('matches the session number', (testContext) => {
    const summaries = describeAll(testContext)

    expect(ids(searchSessionSummaries(summaries, '12'))).toEqual([ 12 ])
  })

  it<Context>('lists nothing when no session matches', (testContext) => {
    expect(searchSessionSummaries(describeAll(testContext), 'nothing like this')).toEqual([])
  })
})

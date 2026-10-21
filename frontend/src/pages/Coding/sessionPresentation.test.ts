// Copyright © 2026 Jalapeno Labs

import type { SessionActivity } from '../../api/routes/codingSessionRoutes'
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

function makeActivity(overrides: Partial<SessionActivity>): SessionActivity {
  return {
    state: 'idle',
    queueDepth: 0,
    currentTurnId: null,
    lastActivityAt: null,
    ...overrides,
  }
}

describe('describeSession', () => {
  beforeEach<Context>((testContext) => {
    // The app's own en-US strings, loaded by the test setup.
    testContext.t = i18next.getFixedT(null, 'coding')
    testContext.context = {
      projectNames: { elysium: 'Elysium' },
      formatInstant: (instant) => `<${instant}>`,
    }
  })

  it<Context>('names the project, state, and last activity', ({ t, context }) => {
    const session = makeCodingSession({
      id: 4,
      projectId: 'elysium',
      activity: makeActivity({ state: 'running', lastActivityAt: '2026-09-20T12:00:00.000Z' }),
    })

    expect(describeSession(session, t, context)).toEqual({
      session,
      projectName: 'Elysium',
      state: 'running',
      stateLabel: 'Running',
      lastActivity: '<2026-09-20T12:00:00.000Z>',
    })
  })

  it<Context>('reads a session that has not run yet as idle with no activity', ({ t, context }) => {
    const summary = describeSession(makeCodingSession({ id: 1, projectId: 'elysium' }), t, context)

    expect(summary.state).toBe('idle')
    expect(summary.stateLabel).toBe('Idle')
    expect(summary.lastActivity).toBe('No activity yet')
  })

  it<Context>('leaves the name blank for a project it does not know', ({ t, context }) => {
    const summary = describeSession(makeCodingSession({ id: 1, projectId: 'gone' }), t, context)

    expect(summary.projectName).toBe('')
  })
})

describe('searchSessionSummaries', () => {
  beforeEach<Context>((testContext) => {
    testContext.t = i18next.getFixedT(null, 'coding')
    testContext.context = {
      projectNames: { elysium: 'Elysium', farworlds: 'Farworlds' },
      formatInstant: (instant) => instant,
    }
  })

  function describeAll({ t, context }: Context) {
    return [
      makeCodingSession({
        id: 12,
        title: 'Fix the login page',
        projectId: 'elysium',
        activity: makeActivity({ state: 'running', lastActivityAt: '2026-09-20T09:00:00.000Z' }),
      }),
      makeCodingSession({
        id: 3,
        title: 'Write the storage docs',
        projectId: 'farworlds',
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

  it<Context>('matches the title, project, and state as shown, ignoring case', (testContext) => {
    const summaries = describeAll(testContext)

    expect(ids(searchSessionSummaries(summaries, 'LOGIN'))).toEqual([ 12 ])
    expect(ids(searchSessionSummaries(summaries, 'farworlds'))).toEqual([ 3 ])
    expect(ids(searchSessionSummaries(summaries, 'running'))).toEqual([ 12 ])
    expect(ids(searchSessionSummaries(summaries, 'idle'))).toEqual([ 3 ])
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

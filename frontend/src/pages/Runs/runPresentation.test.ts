// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { RUN_STATES, STEP_STATES } from '../../api/routes/runRoutes'
import {
  durationBetween,
  nameFromIdentifier,
  runStateLabelKeys,
  stepActionsByState,
  stepStateLabelKeys,
} from './runPresentation'

describe('nameFromIdentifier', () => {
  it('reads a name in code as words', () => {
    expect(nameFromIdentifier('fetch_transcript')).toBe('Fetch transcript')
    expect(nameFromIdentifier('meeting')).toBe('Meeting')
    expect(nameFromIdentifier('_triage__conversation_')).toBe('Triage conversation')
  })

  it('shows a name with no words as it is', () => {
    expect(nameFromIdentifier('___')).toBe('___')
  })
})

describe('durationBetween', () => {
  const started = '2026-11-03T10:00:00Z'

  it('counts seconds, then minutes and seconds, then hours and minutes', () => {
    expect(durationBetween(started, '2026-11-03T10:00:42Z', 0)).toEqual({
      key: 'durations.seconds',
      values: { seconds: 42 },
    })
    expect(durationBetween(started, '2026-11-03T10:03:04Z', 0)).toEqual({
      key: 'durations.minutes',
      values: { minutes: 3, seconds: 4 },
    })
    expect(durationBetween(started, '2026-11-03T12:05:59Z', 0)).toEqual({
      key: 'durations.hours',
      values: { hours: 2, minutes: 5 },
    })
  })

  it('counts up to now while it runs, and shows nothing before it started', () => {
    expect(durationBetween(started, null, Date.parse('2026-11-03T10:00:09Z'))).toEqual({
      key: 'durations.seconds',
      values: { seconds: 9 },
    })
    expect(durationBetween(null, null, Date.now())).toBeNull()
  })
})

describe('stepActionsByState', () => {
  // Mirrors what api/src/models/step_job.rs accepts, so no button asks for a refusal.
  it('offers retry on failed steps, resume on held ones, and skip on any not yet done', () => {
    expect(stepActionsByState.failed).toEqual([ 'retry', 'skip' ])
    expect(stepActionsByState.held).toEqual([ 'resume', 'skip' ])
    expect(stepActionsByState.pending).toEqual([ 'skip' ])
    expect(stepActionsByState.queued).toEqual([ 'skip' ])
    expect(stepActionsByState.running).toEqual([])
    expect(stepActionsByState.succeeded).toEqual([])
    expect(stepActionsByState.skipped).toEqual([])
  })
})

describe('state labels', () => {
  it('names every state a run or a step can be in', () => {
    for (const state of RUN_STATES) {
      expect(i18next.exists(`runs:${runStateLabelKeys[state]}`)).toBe(true)
    }
    for (const state of STEP_STATES) {
      expect(i18next.exists(`runs:${stepStateLabelKeys[state]}`)).toBe(true)
    }
  })
})

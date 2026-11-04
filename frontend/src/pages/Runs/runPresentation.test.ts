// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { RUN_STATES, STEP_STATES } from '../../api/routes/runRoutes'
import {
  nameFromIdentifier,
  runNameKeys,
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

describe('runNameKeys', () => {
  it('has words for every name it maps', () => {
    for (const keys of Object.values(runNameKeys)) {
      for (const key of keys.values()) {
        expect(i18next.exists(`runs:${key}`)).toBe(true)
      }
    }
  })

  // Mirrors the meeting workflow in api/src/meetings/workflow.rs.
  it('names the meeting workflow, its steps, and what its runs are about', () => {
    expect(runNameKeys.workflows.has('meeting')).toBe(true)
    expect(runNameKeys.steps.has('fetch_meeting')).toBe(true)
    expect(runNameKeys.steps.has('transcribe_meeting')).toBe(true)
    expect(runNameKeys.steps.has('correct_transcript')).toBe(true)
    expect(runNameKeys.sources.has('meeting')).toBe(true)
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

// Copyright © 2026 Jalapeno Labs

import type { AgentModel } from '../../../api/routes/modelSettingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { currentChoice, isModelName, pendingChanges } from './modelSettingsPresentation'

const opusHigh = { model: 'claude-opus-5-5', effort: 'high' } as const

function agent(name: string, overrides: Partial<AgentModel> = {}): AgentModel {
  return {
    name,
    kind: 'agent',
    default: opusHigh,
    setting: null,
    ...overrides,
  }
}

describe('isModelName', () => {
  it('takes ids and aliases the CLI takes, as the API does', () => {
    for (const model of [ 'claude-opus-5-5', 'opus', 'opus[1m]', 'claude-sonnet-4.6', 'm'.repeat(100) ]) {
      expect(isModelName(model), model).toBe(true)
    }
  })

  it('refuses anything that could read as a flag or is not a model', () => {
    for (const model of [ '', '--dangerously-skip-permissions', '-x', 'opus 4', 'opus;rm', 'über', 'm'.repeat(101) ]) {
      expect(isModelName(model), model).toBe(false)
    }
  })
})

describe('currentChoice', () => {
  it('is the setting while there is one, and the default otherwise', () => {
    expect(currentChoice(agent('elysia'))).toEqual(opusHigh)
    expect(currentChoice(agent('elysia', {
      setting: { model: 'claude-haiku-5-5', effort: 'low', updatedAt: '2026-11-04T12:00:00Z' },
    }))).toEqual({ model: 'claude-haiku-5-5', effort: 'low' })
  })

  it('is nothing for a step that runs no model', () => {
    expect(currentChoice(agent('transcribe_meeting', { kind: 'step', default: null }))).toBeNull()
  })
})

describe('pendingChanges', () => {
  it('sends only what differs from what each agent runs on now, in the order listed', () => {
    const agents = [
      agent('elysia'),
      agent('coding_session'),
      agent('turn_summary', { default: { model: 'claude-haiku-5-5', effort: 'low' }}),
    ]
    const pending = pendingChanges(agents, {
      turn_summary: { model: 'claude-sonnet-5-5', effort: 'medium' },
      // Picked back to what it runs on: no change.
      coding_session: opusHigh,
      elysia: { model: 'claude-opus-5-5', effort: 'max' },
    })

    expect(pending.map((change) => change.agent.name)).toEqual([ 'elysia', 'turn_summary' ])
    expect(pending[1].choice).toEqual({ model: 'claude-sonnet-5-5', effort: 'medium' })
  })

  it('ignores a change for an agent the list no longer has', () => {
    expect(pendingChanges([ agent('elysia') ], { gone: opusHigh })).toEqual([])
  })
})

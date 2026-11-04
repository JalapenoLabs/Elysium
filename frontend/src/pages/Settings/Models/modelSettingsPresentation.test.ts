// Copyright © 2026 Jalapeno Labs

import type { AgentModel, KindModel } from '../../../api/routes/modelSettingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  currentAgentChoice,
  currentKindChoice,
  isModelName,
  membersByKind,
  pendingChanges,
} from './modelSettingsPresentation'

const opusHigh = { model: 'claude-opus-5-5', effort: 'high' } as const
const haikuLow = { model: 'claude-haiku-5-5', effort: 'low' } as const

function agent(name: string, overrides: Partial<AgentModel> = {}): AgentModel {
  return {
    name,
    isStep: false,
    kind: 'elysia',
    setting: null,
    resolved: { ...opusHigh, source: 'default' },
    ...overrides,
  }
}

function kind(name: KindModel['name'], overrides: Partial<KindModel> = {}): KindModel {
  return {
    name,
    default: opusHigh,
    setting: null,
    runsInProjects: false,
    usage: null,
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

describe('currentKindChoice', () => {
  it('is the admin\'s setting while there is one, and the default otherwise', () => {
    expect(currentKindChoice(kind('summaries', { default: haikuLow }))).toEqual(haikuLow)
    expect(currentKindChoice(kind('worker', {
      setting: { model: 'claude-sonnet-5-5', effort: 'medium', updatedAt: '2026-11-04T12:00:00Z' },
    }))).toEqual({ model: 'claude-sonnet-5-5', effort: 'medium' })
  })
})

describe('currentAgentChoice', () => {
  it('is what the agent resolves to, whichever level it came from', () => {
    expect(currentAgentChoice(agent('coding_session', {
      kind: 'worker',
      resolved: { model: 'claude-sonnet-5-5', effort: 'medium', source: 'kind' },
    }))).toEqual({ model: 'claude-sonnet-5-5', effort: 'medium' })
  })

  it('is nothing for a step that runs no model', () => {
    expect(currentAgentChoice(agent('transcribe_meeting', { isStep: true, kind: null, resolved: null }))).toBeNull()
  })
})

describe('pendingChanges', () => {
  it('sends only what differs from what each row runs on now, in the order listed', () => {
    const agents = [
      agent('elysia'),
      agent('coding_session'),
      agent('turn_summary', { resolved: { ...haikuLow, source: 'default' }}),
    ]
    const pending = pendingChanges(agents, {
      turn_summary: { model: 'claude-sonnet-5-5', effort: 'medium' },
      // Picked back to what it runs on: no change.
      coding_session: opusHigh,
      elysia: { model: 'claude-opus-5-5', effort: 'max' },
    }, currentAgentChoice)

    expect(pending.map((change) => change.row.name)).toEqual([ 'elysia', 'turn_summary' ])
    expect(pending[1].choice).toEqual({ model: 'claude-sonnet-5-5', effort: 'medium' })
  })

  it('ignores a change for a row the list no longer has', () => {
    expect(pendingChanges([ kind('worker') ], { triage: opusHigh }, currentKindChoice)).toEqual([])
  })
})

describe('membersByKind', () => {
  it('lists each kind\'s steps and agents by their words, and leaves out steps that run no model', () => {
    const members = membersByKind([
      agent('coding_session', { kind: 'worker' }),
      agent('transcribe_meeting', { isStep: true, kind: null, resolved: null }),
      agent('studio_session', { kind: 'worker' }),
      agent('elysia'),
    ], (listed) => listed.name.toUpperCase())

    expect(members.get('worker')).toEqual([ 'CODING_SESSION', 'STUDIO_SESSION' ])
    expect(members.get('elysia')).toEqual([ 'ELYSIA' ])
    expect(members.has('triage')).toBe(false)
  })
})

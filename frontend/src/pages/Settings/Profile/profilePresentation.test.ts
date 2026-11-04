// Copyright © 2026 Jalapeno Labs

import type { KratosUi } from '../../../api/kratosFlows'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { readProfile } from './profilePresentation'

function input(name: string, value: string) {
  return {
    type: 'input',
    group: 'profile',
    attributes: { node_type: 'input', name, type: 'text', value, disabled: false },
    messages: [],
    meta: {},
  }
}

// Settings flows arrive as JSON shaped like these.
function flow(nodes: unknown[]) {
  return { id: 'settings-1', ui: { nodes }} as unknown as KratosUi
}

describe('readProfile', () => {
  it('reads the first name, last name, and email Kratos holds', () => {
    const settings = flow([
      input('traits.email', 'ada@example.com'),
      input('traits.name.first', 'Ada'),
      input('traits.name.last', 'Lovelace'),
    ])
    expect(readProfile(settings)).toEqual({ firstName: 'Ada', lastName: 'Lovelace', email: 'ada@example.com' })
  })

  it('splits a name not yet moved to the current schema at its first space', () => {
    const settings = flow([
      input('traits.email', 'gabo@example.com'),
      input('traits.name', ' Gabriel García Márquez '),
    ])
    expect(readProfile(settings)).toEqual({
      firstName: 'Gabriel',
      lastName: 'García Márquez',
      email: 'gabo@example.com',
    })

    const oneName = flow([ input('traits.email', 'cher@example.com'), input('traits.name', 'Cher') ])
    expect(readProfile(oneName)).toEqual({ firstName: 'Cher', lastName: '', email: 'cher@example.com' })
  })
})

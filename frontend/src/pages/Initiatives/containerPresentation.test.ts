// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { EMPTY_CONTAINER_TARGET_DRAFT, toContainerTarget } from './containerPresentation'

describe('toContainerTarget', () => {
  it('names nothing until a container is picked', () => {
    expect(toContainerTarget({ ...EMPTY_CONTAINER_TARGET_DRAFT, credentialKey: 'jira:j1' })).toBeNull()
  })

  it('names a picked Jira epic through its credential', () => {
    const target = toContainerTarget({
      ...EMPTY_CONTAINER_TARGET_DRAFT,
      credentialKey: 'jira:j1',
      kind: 'epic',
      reference: 'ELY-7',
    })
    expect(target).toEqual({ provider: 'jira', credentialId: 'j1', kind: 'epic', reference: 'ELY-7' })
  })

  it('refuses a kind the provider does not have', () => {
    const target = toContainerTarget({
      ...EMPTY_CONTAINER_TARGET_DRAFT,
      credentialKey: 'github:g1',
      kind: 'epic',
      reference: 'acme/elysium#3',
    })
    expect(target).toBeNull()
  })
})

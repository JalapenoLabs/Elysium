// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { describeActor } from './actionItemPresentation'

const names = {
  namesById: { 'ada-id': 'Ada Lovelace', 'grace-id': 'Grace Hopper' },
  meId: 'ada-id',
}

describe('describeActor', () => {
  it('names a person by their name, and the reader as you', () => {
    expect(describeActor('user:grace-id', names)).toEqual({ key: 'actors.person', values: { detail: 'Grace Hopper' }})
    expect(describeActor('user:ada-id', names)).toEqual({ key: 'actors.you', values: { detail: '' }})
  })

  it('says someone for a person whose name has not loaded', () => {
    expect(describeActor('user:unknown-id', names).key).toBe('actors.someone')
  })

  it('keeps rows written before accounts readable', () => {
    expect(describeActor('user', names).key).toBe('actors.beforeAccounts')
  })

  it('names machines by their kind and detail', () => {
    expect(describeActor('elysia')).toEqual({ key: 'actors.elysia', values: { detail: '' }})
    expect(describeActor('session:12')).toEqual({ key: 'actors.session', values: { detail: '12' }})
    expect(describeActor('watcher:jira')).toEqual({ key: 'actors.watcher', values: { detail: 'jira' }})
  })

  it('shows an actor this build does not know as sent', () => {
    expect(describeActor('robot:7')).toEqual({ key: 'actors.unknown', values: { detail: 'robot:7' }})
  })
})

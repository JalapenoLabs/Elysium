// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { llmsLoaded, llmUsageUpdated, selectLlmById } from './llmsSlice'

// Misc
import { makeClaudeAccount, makeLlm } from '../testFixtures'

describe('llmUsageUpdated', () => {
  it('replaces the usage of a signed-in account and nothing else', () => {
    const store = createAppStore()
    store.dispatch(llmsLoaded([
      makeLlm({ id: 'account', type: 'claude-account', account: makeClaudeAccount() }),
    ]))
    const usage = {
      fiveHour: { utilization: 64, resetsAt: '2026-10-21T05:00:00Z' },
      sevenDay: null,
      readAt: '2026-10-21T02:00:00Z',
    }

    store.dispatch(llmUsageUpdated({ id: 'account', usage }))

    const account = selectLlmById(store.getState(), 'account')?.account
    expect(account?.usage).toEqual(usage)
    expect(account?.email).toBe('alex@example.com')
  })

  it('leaves a credential without an account untouched', () => {
    const store = createAppStore()
    store.dispatch(llmsLoaded([ makeLlm({ id: 'key' }) ]))

    store.dispatch(llmUsageUpdated({
      id: 'key',
      usage: { fiveHour: null, sevenDay: null, readAt: '2026-10-21T02:00:00Z' },
    }))

    expect(selectLlmById(store.getState(), 'key')?.account).toBeNull()
  })
})

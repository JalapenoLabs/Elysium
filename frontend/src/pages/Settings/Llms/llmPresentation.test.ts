// Copyright © 2026 Jalapeno Labs

import type { Llm, LlmType } from '../../../api/routes/llmRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getLlmStatus, hasAgentCredential } from './llmPresentation'

const NOW = Date.parse('2026-10-21T02:00:00Z')

function llm(type: LlmType, overrides: Partial<Llm> = {}): Llm {
  return {
    id: `${type}-id`,
    name: type,
    description: '',
    type,
    priority: 0,
    isActive: true,
    expiresAt: null,
    createdAt: '2026-10-01T00:00:00Z',
    createdBy: 'user-id',
    updatedAt: '2026-10-01T00:00:00Z',
    ...overrides,
  }
}

describe('getLlmStatus', () => {
  it('reports expiry over the active flag', () => {
    expect(getLlmStatus(llm('claude-api-token', { expiresAt: '2026-10-20T00:00:00Z' }), NOW)).toBe('expired')
    expect(getLlmStatus(llm('claude-api-token', { isActive: false }), NOW)).toBe('inactive')
    expect(getLlmStatus(llm('claude-api-token', { expiresAt: '2026-11-01T00:00:00Z' }), NOW)).toBe('active')
  })
})

// A session started with no Claude credential ran its turn anyway and failed with the CLI's
// "Not logged in · Please run /login", which nothing in Elysium can act on. The New session
// dialog now warns first, using this check.
describe('hasAgentCredential', () => {
  it('is false with no credentials at all', () => {
    expect(hasAgentCredential([], NOW)).toBe(false)
  })

  it('is true for an active Claude subscription token or API key', () => {
    expect(hasAgentCredential([ llm('claude-code-oauth') ], NOW)).toBe(true)
    expect(hasAgentCredential([ llm('claude-api-token') ], NOW)).toBe(true)
  })

  it('ignores OpenAI credentials, which agent turns cannot run on', () => {
    expect(hasAgentCredential([ llm('chatgpt-oauth'), llm('chatgpt-api-token') ], NOW)).toBe(false)
  })

  it('ignores inactive and expired Claude credentials', () => {
    const inactive = llm('claude-code-oauth', { isActive: false })
    const expired = llm('claude-api-token', { expiresAt: '2026-10-21T01:59:59Z' })
    expect(hasAgentCredential([ inactive, expired ], NOW)).toBe(false)
  })
})

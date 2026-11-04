// Copyright © 2026 Jalapeno Labs

import type { IssueReportCredential, IssueReportPreview } from '../api/routes/issueReportRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { canTryToken, getReportBlocker, resolveReportCredential } from './issueReportPresentation'

function credential(id: string, readiness: IssueReportCredential['readiness']): IssueReportCredential {
  return {
    id,
    name: id,
    kind: 'classic',
    login: 'ada',
    isDefault: false,
    readiness,
  }
}

function preview(overrides: Partial<IssueReportPreview> = {}): IssueReportPreview {
  return {
    draft: { title: 'Title', body: 'Body', fingerprint: '0123456789abcdef' },
    repository: 'JalapenoLabs/Elysium',
    credentials: [ credential('ready', 'ready') ],
    suggestedCredentialId: 'ready',
    previousReport: null,
    remainingToday: 10,
    ...overrides,
  }
}

describe('canTryToken', () => {
  it('tries a ready or fine-grained token and never one GitHub will refuse', () => {
    expect(canTryToken('ready')).toBe(true)
    expect(canTryToken('fine_grained')).toBe(true)
    expect(canTryToken('missing_scope')).toBe(false)
    expect(canTryToken('expired')).toBe(false)
  })
})

describe('getReportBlocker', () => {
  it('waits for the preview of what was typed before anything can be posted', () => {
    expect(getReportBlocker(undefined, null, true)).toBe('loading')
    expect(getReportBlocker(preview(), credential('ready', 'ready'), false)).toBe('loading')
  })

  it('asks for a token of their own when the person has none', () => {
    expect(getReportBlocker(preview({ credentials: [], suggestedCredentialId: null }), null, true))
      .toBe('noToken')
  })

  it('stops at the daily limit', () => {
    expect(getReportBlocker(preview({ remainingToday: 0 }), credential('ready', 'ready'), true))
      .toBe('limitReached')
  })

  it('refuses a token that cannot report and allows one that can', () => {
    expect(getReportBlocker(preview(), credential('old', 'expired'), true)).toBe('tokenCannotReport')
    expect(getReportBlocker(preview(), null, true)).toBe('tokenCannotReport')
    expect(getReportBlocker(preview(), credential('ready', 'ready'), true)).toBeNull()
  })
})

describe('resolveReportCredential', () => {
  const tokens = [ credential('first', 'ready'), credential('second', 'fine_grained') ]

  it('keeps the pick, and otherwise follows the suggestion', () => {
    const shown = preview({ credentials: tokens, suggestedCredentialId: 'first' })
    expect(resolveReportCredential(shown, 'second')?.id).toBe('second')
    expect(resolveReportCredential(shown, null)?.id).toBe('first')
  })

  it('drops a pick that is no longer among the tokens', () => {
    const shown = preview({ credentials: tokens, suggestedCredentialId: 'first' })
    expect(resolveReportCredential(shown, 'deleted')?.id).toBe('first')
    expect(resolveReportCredential(undefined, 'first')).toBeNull()
  })
})

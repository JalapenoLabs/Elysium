// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { DRAFT_STORAGE_PREFIX } from '../constants'
import { forgetSessionDraft, keepSessionDraft, readSessionDraft } from './sessionDrafts'

const draft = {
  title: 'Pin every tool version',
  category: 'convention',
  body: 'Never use latest.',
  evidence: '',
} as const

describe('sessionDrafts', () => {
  beforeEach(() => {
    window.sessionStorage.clear()
    vi.restoreAllMocks()
  })

  it('gives back what was kept until it is forgotten', () => {
    keepSessionDraft('learning', draft, [ draft.title, draft.body, draft.evidence ])
    expect(readSessionDraft('learning')).toEqual(draft)

    forgetSessionDraft('learning')
    expect(readSessionDraft('learning')).toBeNull()
  })

  it('keeps a box of text as it was typed, line breaks and all', () => {
    keepSessionDraft('reply.record-1', 'Friday works.\nSee you then.', [ 'Friday works.\nSee you then.' ])
    expect(readSessionDraft('reply.record-1')).toBe('Friday works.\nSee you then.')
  })

  it('keeps each draft apart by name', () => {
    keepSessionDraft('reply.record-1', 'First', [ 'First' ])
    keepSessionDraft('reply.record-2', 'Second', [ 'Second' ])

    forgetSessionDraft('reply.record-1')

    expect(readSessionDraft('reply.record-1')).toBeNull()
    expect(readSessionDraft('reply.record-2')).toBe('Second')
  })

  it('never writes a draft without text over one with it', () => {
    keepSessionDraft('learning', draft, [ draft.title, draft.body, draft.evidence ])
    keepSessionDraft('learning', { title: ' ', category: 'decision', body: '', evidence: '\n' }, [ ' ', '', '\n' ])
    expect(readSessionDraft('learning')).toEqual(draft)

    keepSessionDraft('reply.record-1', 'Half a thought', [ 'Half a thought' ])
    keepSessionDraft('reply.record-1', '   ', [ '   ' ])
    expect(readSessionDraft('reply.record-1')).toBe('Half a thought')
  })

  it('never writes a choice on its own, with nothing typed', () => {
    keepSessionDraft('action-item', { title: '', priority: 'high' }, [ '' ])
    expect(readSessionDraft('action-item')).toBeNull()
  })

  it('reads a draft it cannot parse as none, rather than failing the form', () => {
    window.sessionStorage.setItem(`${DRAFT_STORAGE_PREFIX}learning`, '{ not json')
    expect(readSessionDraft('learning')).toBeNull()
  })

  it('lets the form work when storage refuses to keep anything', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('The quota has been exceeded.', 'QuotaExceededError')
    })

    expect(() => keepSessionDraft('learning', draft, [ draft.title, draft.body, draft.evidence ])).not.toThrow()
    expect(readSessionDraft('learning')).toBeNull()
  })
})

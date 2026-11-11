// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { PageHistoryEntry } from '../../../api/routes/pageRoutes'

// Core
import { describe, expect, it, vi } from 'vitest'

// Misc
import i18next from 'i18next'
import { makePageHistoryEntry } from '../../../testFixtures'
import { describePageHistory, pageAgentName, readPageHistoryView } from './pageHistoryPresentation'

function describeOne(overrides: Partial<PageHistoryEntry>) {
  return describePageHistory([ makePageHistoryEntry({ id: 'entry', ...overrides }) ])[0]
}

describe('describePageHistory', () => {
  it('reads newest first', () => {
    const rows = describePageHistory([
      makePageHistoryEntry({ id: 'first', kind: 'created', data: { parentId: null, version: 1 }}),
      makePageHistoryEntry({ id: 'second', data: { version: 2 }}),
    ])

    expect(rows.map((row) => row.entry.id)).toEqual([ 'second', 'first' ])
  })

  it('treats a page\'s creation as its first version', () => {
    const row = describeOne({ kind: 'created', data: { parentId: null, version: 1 }})

    expect(row).toMatchObject({ sentence: 'created', version: 1 })
  })

  it('names the version a restore copied', () => {
    const row = describeOne({ data: { version: 4, restoredFrom: 2 }})

    expect(row).toMatchObject({ sentence: 'restored', version: 4, otherVersion: 2 })
  })

  it('names the version an undo reversed', () => {
    const rows = describePageHistory([
      makePageHistoryEntry({ id: 'revised', data: { version: 2 }}),
      makePageHistoryEntry({ id: 'undo', data: { version: 3, undid: 'revised' }}),
    ])

    expect(rows[0]).toMatchObject({ sentence: 'undid', version: 3, otherVersion: 2 })
  })

  it('still reads an undo of something that wrote no version', () => {
    const row = describeOne({ data: { version: 3, undid: 'a-deletion' }})

    expect(row).toMatchObject({ sentence: 'undidEarlier', otherVersion: null })
  })

  it('says a version came from an accepted suggestion', () => {
    expect(describeOne({ data: { version: 3, suggestionId: 'suggestion' }}).sentence).toBe('accepted')
  })

  it('reads a move as where the page went', () => {
    const row = describeOne({
      kind: 'moved',
      data: { from: { parentId: null, after: null }, to: { parentId: 'parent', after: 'sibling' }},
    })

    expect(row).toMatchObject({
      sentence: 'moved',
      version: null,
      move: { fromParentId: null, toParentId: 'parent' },
    })
  })

  it('tells a change of place among siblings from a move', () => {
    const row = describeOne({
      kind: 'moved',
      data: { from: { parentId: 'parent', after: null }, to: { parentId: 'parent', after: 'sibling' }},
    })

    expect(row.sentence).toBe('reordered')
  })

  it('still reads a move of a shape it does not know', () => {
    vi.spyOn(console, 'debug').mockImplementationOnce(() => {})

    expect(describeOne({ kind: 'moved', data: { from: 'somewhere' }})).toMatchObject({
      sentence: 'changed',
      move: null,
    })
  })

  it('says whether a deletion or restore came from a page above', () => {
    expect(describeOne({ kind: 'deleted', data: {}}).sentence).toBe('deleted')
    expect(describeOne({ kind: 'attached', data: { attachmentId: 'file', name: 'plan.pdf' }})).toMatchObject({
      sentence: 'attached',
      attachment: { id: 'file', name: 'plan.pdf' },
    })
    expect(describeOne({ kind: 'detached', data: { attachmentId: 'file', name: 'plan.pdf' }})).toMatchObject({
      sentence: 'detached',
      attachment: { id: 'file', name: 'plan.pdf' },
    })
    expect(describeOne({ kind: 'detached', data: {}})).toMatchObject({ sentence: 'changed', attachment: null })
    expect(describeOne({ kind: 'deleted', data: { deletedWith: 'parent' }}).sentence).toBe('deletedWith')
    expect(describeOne({ kind: 'restored', data: { deletedAt: 'then' }}).sentence).toBe('broughtBack')
    expect(describeOne({ kind: 'restored', data: { restoredWith: 'parent' }}).sentence).toBe('broughtBackWith')
  })

  it('reads a deletion and restore carried over from a learning, which hold nothing', () => {
    expect(describeOne({ kind: 'deleted', data: {}}).version).toBeNull()
    expect(describeOne({ kind: 'restored', data: {}}).sentence).toBe('broughtBack')
  })

  it('names the label a change put on or took off, which writes no version', () => {
    expect(describeOne({ kind: 'labeled', data: { label: 'runbook' }})).toMatchObject({
      sentence: 'labeled',
      label: 'runbook',
      version: null,
    })
    expect(describeOne({ kind: 'unlabeled', data: { label: 'runbook' }})).toMatchObject({
      sentence: 'unlabeled',
      label: 'runbook',
      version: null,
    })
  })

  it('reads a label entry of an unknown shape as a change', () => {
    expect(describeOne({ kind: 'labeled', data: {}}).sentence).toBe('changed')
  })

  it('names the project a change linked or took it off, as it was called then', () => {
    const data = { projectId: 'launch', projectName: 'Launch' }

    expect(describeOne({ kind: 'project_linked', data })).toMatchObject({
      sentence: 'projectLinked',
      project: 'Launch',
      version: null,
    })
    expect(describeOne({ kind: 'project_unlinked', data })).toMatchObject({
      sentence: 'projectUnlinked',
      project: 'Launch',
      version: null,
    })
  })

  it('reads a project entry of an unknown shape as a change', () => {
    expect(describeOne({ kind: 'project_linked', data: {}}).sentence).toBe('changed')
  })

  it('still reads a kind a later API writes as a change', () => {
    expect(describeOne({ kind: 'labelled', data: { label: 'Finance' }})).toMatchObject({
      sentence: 'changed',
      version: null,
    })
  })
})

describe('readPageHistoryView', () => {
  it('shows the list by default', () => {
    expect(readPageHistoryView(new URLSearchParams('tab=history'))).toEqual({ kind: 'list' })
  })

  it('reads one version', () => {
    expect(readPageHistoryView(new URLSearchParams('version=3'))).toEqual({ kind: 'version', version: 3 })
  })

  it('compares older against newer, whichever way round they are named', () => {
    const expected = { kind: 'compare', from: 3, to: 9 }

    expect(readPageHistoryView(new URLSearchParams('from=3&to=9'))).toEqual(expected)
    expect(readPageHistoryView(new URLSearchParams('from=9&to=3'))).toEqual(expected)
  })

  it('reads a version compared with itself as that version', () => {
    expect(readPageHistoryView(new URLSearchParams('from=3&to=3'))).toEqual({ kind: 'version', version: 3 })
  })

  it('shows the list for an address that names nothing readable', () => {
    for (const search of [ 'version=two', 'version=0', 'version=-1', 'version=1.5', 'from=2' ]) {
      expect(readPageHistoryView(new URLSearchParams(search))).toEqual({ kind: 'list' })
    }
  })
})

describe('pageAgentName', () => {
  const t: TFunction<'knowledge'> = i18next.getFixedT('en-US', 'knowledge')

  it('uses what an agent called itself', () => {
    expect(pageAgentName({ id: 'mcp:client', name: 'Claude Code' }, t)).toBe('Claude Code')
  })

  it('names a session by its number', () => {
    expect(pageAgentName({ id: 'session:12', name: null }, t)).toBe('session 12')
  })

  it('names a Project Manager', () => {
    expect(pageAgentName({ id: 'project_manager:project', name: null }, t)).toBe('the Project Manager')
  })

  it('says an agent when nothing names it', () => {
    expect(pageAgentName({ id: 'mcp:client', name: null }, t)).toBe('an agent')
  })
})

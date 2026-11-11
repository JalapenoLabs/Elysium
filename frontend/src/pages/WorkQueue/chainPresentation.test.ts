// Copyright © 2026 Jalapeno Labs

import type { WorkBatchLink } from '../../api/routes/workQueueRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { chainLinkKey, chainLinkTarget, chainLinkText } from './chainPresentation'

const SEGMENT: WorkBatchLink = {
  kind: 'record',
  id: 'record-1',
  sourceKind: 'meeting',
  title: 'Annual pricing',
  occurredAt: '2026-11-04T15:00:00Z',
  excerpt: '[00:12] Ada: We need annual pricing',
  meetingTitle: 'Weekly sync',
  documentId: 'document-1',
}

const ISSUE: WorkBatchLink = {
  kind: 'issue',
  actionItemId: 'item-1',
  provider: 'jira',
  key: 'ELY-12',
  url: 'https://acme.atlassian.net/browse/ELY-12',
  title: 'Annual pricing',
}

const PULL_REQUEST: WorkBatchLink = {
  kind: 'pullRequest',
  id: 'pull-1',
  repository: 'acme/web',
  number: 7,
  url: 'https://github.com/acme/web/pull/7',
  title: 'Add annual pricing',
  state: 'open',
}

const FINDING: WorkBatchLink = {
  kind: 'finding',
  id: 'finding-1',
  title: 'The login page leaks its token',
  acceptedById: 'person-1',
  acceptedBy: 'Alex',
  acceptedAt: '2026-11-04T15:00:00Z',
}

const MESSAGE: WorkBatchLink = {
  kind: 'message',
  id: 'message-1',
  authorId: 'person-1',
  author: 'Alex',
  body: 'Ship annual pricing this week.',
  sentAt: '2026-11-04T15:00:00Z',
}

describe('chainLinkText', () => {
  it('reads a meeting segment under its meeting, and any other record by its title', () => {
    expect(chainLinkText(SEGMENT)).toBe('Weekly sync: Annual pricing')
    expect(chainLinkText({ ...SEGMENT, meetingTitle: null })).toBe('Annual pricing')
  })

  it('reads an issue by its key and title, and a pull request by its reference', () => {
    expect(chainLinkText(ISSUE)).toBe('ELY-12 Annual pricing')
    expect(chainLinkText(PULL_REQUEST)).toBe('acme/web#7')
  })

  it('reads a message by what it said and a finding by what was seen', () => {
    expect(chainLinkText(MESSAGE)).toBe('Ship annual pricing this week.')
    expect(chainLinkText(FINDING)).toBe('The login page leaks its token')
  })
})

describe('chainLinkTarget', () => {
  it('opens a record at its knowledge base document, and nowhere before it is indexed', () => {
    expect(chainLinkTarget(SEGMENT, 'project-1')).toEqual({
      href: '/knowledge/documents/document-1',
      isExternal: false,
    })
    expect(chainLinkTarget({ ...SEGMENT, documentId: null }, 'project-1')).toBeNull()
  })

  it('opens an issue in its tracker and a pull request on GitHub, outside Elysium', () => {
    expect(chainLinkTarget(ISSUE, 'project-1')).toEqual({ href: ISSUE.url, isExternal: true })
    expect(chainLinkTarget(PULL_REQUEST, 'project-1')).toEqual({ href: PULL_REQUEST.url, isExternal: true })
  })

  it('opens a message and a finding on the batch\'s project, and an item and a batch on their pages', () => {
    expect(chainLinkTarget(MESSAGE, 'project-1')?.href).toBe('/projects/project-1?tab=manager')
    expect(chainLinkTarget(FINDING, 'project-1')?.href).toBe('/projects/project-1?tab=findings')
    const item: WorkBatchLink = { kind: 'item', id: 'item-1', title: 'Price the plans', state: 'open' }
    expect(chainLinkTarget(item, 'project-1')?.href).toBe('/action-items/item-1')
    const batch: WorkBatchLink = { kind: 'batch', id: 'batch-1', projectId: 'project-1', title: 'Pricing' }
    expect(chainLinkTarget(batch, 'project-1')?.href).toBe('/work-queue/batch-1')
  })
})

describe('chainLinkKey', () => {
  it('keeps one issue linked to two items apart, and two kinds sharing an id apart', () => {
    const otherItem: WorkBatchLink = { ...ISSUE, actionItemId: 'item-2' }
    expect(chainLinkKey(ISSUE)).not.toBe(chainLinkKey(otherItem))

    const item: WorkBatchLink = { kind: 'item', id: 'shared', title: 'Price the plans', state: 'open' }
    const batch: WorkBatchLink = { kind: 'batch', id: 'shared', projectId: 'project-1', title: 'Pricing' }
    expect(chainLinkKey(item)).not.toBe(chainLinkKey(batch))
  })
})

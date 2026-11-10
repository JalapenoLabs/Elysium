// Copyright © 2026 Jalapeno Labs

import type { WorkBatchLink, WorkBatchWhy as Why } from '../../api/routes/workQueueRoutes'

// Core
import { render, screen } from '@testing-library/react'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { describe, expect, it } from 'vitest'

// User interface
import { WorkBatchWhy } from './WorkBatchWhy'

// Misc
import { TestAriaRouter } from '../../testRouter'

const BATCH: WorkBatchLink = {
  kind: 'batch',
  id: 'batch-1',
  projectId: 'project-1',
  title: 'Ship annual pricing',
}

function renderWhy(chain: WorkBatchLink[]) {
  const why: Why = { chain, parents: []}
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>
      <WorkBatchWhy why={why} batchId='batch-1' projectId='project-1' />
    </TestAriaRouter>,
  }])
  render(<RouterProvider router={router} />)
}

describe('WorkBatchWhy', () => {
  it('reads from the meeting quote and the issue to the item, the batch, and its pull request', () => {
    renderWhy([
      {
        kind: 'record',
        id: 'record-1',
        sourceKind: 'meeting',
        title: 'Annual pricing',
        occurredAt: '2026-11-04T15:00:00Z',
        excerpt: '[00:12] Ada: We need annual pricing',
        meetingTitle: 'Weekly sync',
        documentId: 'document-1',
      },
      {
        kind: 'issue',
        actionItemId: 'item-1',
        provider: 'jira',
        key: 'ELY-12',
        url: 'https://acme.atlassian.net/browse/ELY-12',
        title: 'Annual pricing',
      },
      { kind: 'item', id: 'item-1', title: 'Price the plans', state: 'open' },
      BATCH,
      {
        kind: 'pullRequest',
        id: 'pull-1',
        repository: 'acme/web',
        number: 7,
        url: 'https://github.com/acme/web/pull/7',
        title: 'Annual pricing',
        state: 'open',
      },
    ])

    const meeting = screen.getByRole('link', { name: 'Weekly sync: Annual pricing' })
    expect(meeting.getAttribute('href')).toBe('/knowledge/documents/document-1')
    expect(screen.getByText('[00:12] Ada: We need annual pricing')).toBeTruthy()
    const issue = screen.getByRole('link', { name: 'ELY-12 Annual pricing' })
    expect(issue.getAttribute('href')).toBe('https://acme.atlassian.net/browse/ELY-12')
    expect(issue.getAttribute('target')).toBe('_blank')
    expect(screen.getByRole('link', { name: 'Price the plans' }).getAttribute('href')).toBe('/action-items/item-1')
    expect(screen.getByText('Ship annual pricing (this batch)')).toBeTruthy()
    const pullRequest = screen.getByRole('link', { name: 'acme/web#7' })
    expect(pullRequest.getAttribute('href')).toBe('https://github.com/acme/web/pull/7')

    const kinds = screen.getAllByRole('listitem').map((item) => item.textContent ?? '')
    expect(kinds[0].startsWith('Meeting')).toBe(true)
    expect(kinds[1].startsWith('Issue')).toBe(true)
    expect(kinds[2].startsWith('Action item')).toBe(true)
    expect(kinds[3].startsWith('Work batch')).toBe(true)
    expect(kinds[4].startsWith('Pull request')).toBe(true)
  })

  it('quotes the message a person sent its Project Manager, saying who sent it', () => {
    renderWhy([
      {
        kind: 'message',
        id: 'message-1',
        authorId: 'person-1',
        author: 'Alex',
        body: 'Ship annual pricing this week.',
        sentAt: '2026-11-04T15:00:00Z',
      },
      BATCH,
    ])

    const message = screen.getByRole('link', { name: 'Ship annual pricing this week.' })
    expect(message.getAttribute('href')).toBe('/projects/project-1?tab=manager')
    expect(screen.getByText(/^From Alex, /)).toBeTruthy()
  })

  it('shows the finding found work came from and who accepted it', () => {
    renderWhy([
      {
        kind: 'finding',
        id: 'finding-1',
        title: 'The login page leaks its token',
        acceptedById: 'person-1',
        acceptedBy: 'Alex',
        acceptedAt: '2026-11-04T15:00:00Z',
      },
      BATCH,
    ])

    const finding = screen.getByRole('link', { name: 'The login page leaks its token' })
    expect(finding.getAttribute('href')).toBe('/projects/project-1?tab=findings')
    expect(screen.getByText(/^Accepted by Alex, /)).toBeTruthy()
  })

  it('names a record the knowledge base has not indexed yet without linking it', () => {
    renderWhy([
      {
        kind: 'record',
        id: 'record-1',
        sourceKind: 'slack',
        title: 'Can we bill yearly?',
        occurredAt: '2026-11-04T15:00:00Z',
        excerpt: '',
        meetingTitle: null,
        documentId: null,
      },
      BATCH,
    ])

    expect(screen.getByText('Can we bill yearly?')).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Can we bill yearly?' })).toBeNull()
    expect(screen.getAllByRole('listitem')[0].textContent?.startsWith('Slack')).toBe(true)
  })

  it('leaves out a link of a kind this build does not know, and draws the rest', () => {
    // As an older bundle reads a newer API's answer.
    const unknown: WorkBatchLink = JSON.parse('{ "kind": "meetingNote", "id": "note-1", "title": "Pricing" }')
    renderWhy([ unknown, BATCH ])

    const kinds = screen.getAllByRole('listitem').map((item) => item.textContent ?? '')
    expect(kinds).toHaveLength(1)
    expect(kinds[0].startsWith('Work batch')).toBe(true)
  })

  it('says when its Project Manager named nothing', () => {
    renderWhy([ BATCH ])

    expect(screen.getByText('Its Project Manager named nothing that called for it.')).toBeTruthy()
  })
})

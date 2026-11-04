// Copyright © 2026 Jalapeno Labs

import type { IssueReportPreview, IssueReportSubject } from '../api/routes/issueReportRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { SWRConfig } from 'swr'

// User interface
import { ReportIssueGate } from '../gates/ReportIssueGate'

// Misc
import { useReportIssue } from '../hooks/useReportIssue'

// The two requests are the network boundary; everything between them and the screen is real.
const routes = vi.hoisted(() => ({
  previewIssueReport: vi.fn(),
  fileIssueReport: vi.fn(),
}))

vi.mock('../api/routes/issueReportRoutes', () => routes)

const SUBJECT: IssueReportSubject = { kind: 'connectorCheck', connectorId: 'connector-1', checkId: 'sign_in' }

function preview(overrides: Partial<IssueReportPreview> = {}): IssueReportPreview {
  return {
    draft: {
      title: 'The `github` connector\'s `sign_in` check failed: GitHub refused [email]',
      body: '### What happened\n\n```text\nGitHub refused [email]\n```\n',
      fingerprint: '0123456789abcdef',
    },
    repository: 'JalapenoLabs/Elysium',
    credentials: [{
      id: 'token-1',
      name: 'Work',
      kind: 'classic',
      login: 'ada',
      isDefault: true,
      readiness: 'ready',
    }],
    suggestedCredentialId: 'token-1',
    previousReport: null,
    remainingToday: 10,
    ...overrides,
  }
}

function Opener() {
  const reportIssue = useReportIssue()
  return <button type='button' onClick={() => reportIssue(SUBJECT)}>Open</button>
}

// The button itself, not the tooltip's trigger around it, which shares its name.
function postButton() {
  const button = screen.getByText('Post to GitHub').closest('button')
  if (!button) {
    throw new Error('Post to GitHub is not a button')
  }
  return button
}

function renderGate() {
  return render(<SWRConfig value={{ provider: () => new Map() }}>
    <MemoryRouter>
      <ReportIssueGate>
        <Opener />
      </ReportIssueGate>
    </MemoryRouter>
  </SWRConfig>)
}

describe('ReportIssueModal', () => {
  beforeEach(() => {
    routes.previewIssueReport.mockReset()
    routes.fileIssueReport.mockReset()
  })

  it('shows exactly what the API rendered and posts it with the suggested token', async () => {
    routes.previewIssueReport.mockResolvedValue(preview())
    routes.fileIssueReport.mockResolvedValue({
      report: {
        filedAs: 'issue',
        issueNumber: 12,
        issueUrl: 'https://github.com/JalapenoLabs/Elysium/issues/12',
        url: 'https://github.com/JalapenoLabs/Elysium/issues/12',
      },
    })
    renderGate()

    fireEvent.click(screen.getByText('Open'))
    const shown = preview().draft
    await waitFor(() => expect(document.querySelector('pre')?.textContent)
      .toBe(`${shown.title}\n\n${shown.body}`))
    expect(routes.previewIssueReport).toHaveBeenCalledWith({ subject: SUBJECT, note: undefined })
    expect(screen.getByText('Filed on JalapenoLabs/Elysium as @ada.')).toBeTruthy()

    const post = postButton()
    await waitFor(() => expect(post.hasAttribute('disabled')).toBe(false))
    fireEvent.click(post)
    await waitFor(() => expect(routes.fileIssueReport).toHaveBeenCalledWith({
      subject: SUBJECT,
      note: undefined,
      credentialId: 'token-1',
    }))
  })

  it('offers adding a token instead of posting when the person has none of their own', async () => {
    routes.previewIssueReport.mockResolvedValue(preview({ credentials: [], suggestedCredentialId: null }))
    renderGate()

    fireEvent.click(screen.getByText('Open'))
    const add = await screen.findByText('Add a GitHub token')
    expect(add.closest('a')?.getAttribute('href')).toBe('/settings/connectors/new/github')
    expect(postButton().hasAttribute('disabled')).toBe(true)
  })

  it('links to the report the person already made of this problem', async () => {
    routes.previewIssueReport.mockResolvedValue(preview({
      previousReport: {
        issueNumber: 12,
        issueUrl: 'https://github.com/JalapenoLabs/Elysium/issues/12',
        url: 'https://github.com/JalapenoLabs/Elysium/issues/12#issuecomment-3',
        filedAs: 'comment',
        reportedAt: '2026-11-02T10:00:00Z',
      },
    }))
    renderGate()

    fireEvent.click(screen.getByText('Open'))
    const previous = await screen.findByText('View your report')
    expect(previous.closest('a')?.getAttribute('href'))
      .toBe('https://github.com/JalapenoLabs/Elysium/issues/12#issuecomment-3')
  })
})

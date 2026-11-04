// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

// User interface
import { AccountFields } from './AccountFields'
import { CheckList } from './CheckList'

const SPECS = [
  { id: 'sign_in', required: true, dependsOn: []},
  { id: 'list_projects', required: true, dependsOn: [ 'sign_in' ]},
  { id: 'sample_issues', required: false, dependsOn: [ 'list_projects' ]},
]

const NOTHING = { count: null, reason: null, message: null, blockedBy: null }

describe('CheckList', () => {
  it('shows each check in order with what it found and why the rest waited', () => {
    render(<CheckList
      kind='jira'
      specs={SPECS}
      outcomes={[
        { ...NOTHING, checkId: 'sign_in', state: 'passed' },
        { ...NOTHING, checkId: 'list_projects', state: 'failed', message: 'Jira refused the token' },
        { ...NOTHING, checkId: 'sample_issues', state: 'skipped', blockedBy: 'list_projects' },
      ]}
    />)

    // Each row's first line is its check's name.
    const labels = screen
      .getAllByRole('listitem')
      .map((row) => row.querySelector('p')?.textContent)
    expect(labels).toEqual([ 'Sign in', 'List projects', 'Sample issues' ])
    expect(screen.getByText('Jira refused the token')).toBeTruthy()
    expect(screen.getByText('Waits for List projects')).toBeTruthy()
    expect(screen.getByText('3 of 3')).toBeTruthy()
    expect(screen.getAllByText('Required')).toHaveLength(2)
  })

  it('counts a running check as not done', () => {
    render(<CheckList
      kind='jira'
      specs={SPECS}
      outcomes={[
        { ...NOTHING, checkId: 'sign_in', state: 'passed' },
        { ...NOTHING, checkId: 'list_projects', state: 'running' },
      ]}
    />)
    expect(screen.getByText('1 of 3')).toBeTruthy()
  })

  it('offers a report beside each failed check alone, naming the check', () => {
    const reportSubjectOf = vi.fn((checkId: string) => ({ kind: 'checkRun' as const, runId: 'run-1', checkId }))
    render(<CheckList
      kind='jira'
      specs={SPECS}
      outcomes={[
        { ...NOTHING, checkId: 'sign_in', state: 'passed' },
        { ...NOTHING, checkId: 'list_projects', state: 'failed', message: 'Jira refused the token' },
        { ...NOTHING, checkId: 'sample_issues', state: 'skipped', blockedBy: 'list_projects' },
      ]}
      reportSubjectOf={reportSubjectOf}
    />)

    expect(screen.getAllByRole('button', { name: 'Report this problem to Elysium\'s developers' })).toHaveLength(1)
    expect(reportSubjectOf).toHaveBeenCalledWith('list_projects')
    expect(reportSubjectOf).not.toHaveBeenCalledWith('sign_in')
  })
})

describe('AccountFields', () => {
  it('draws a field for each of the definition\'s, with a secret as a password', () => {
    const onChange = vi.fn()
    render(<AccountFields
      definition={{
        kind: 'atlassian',
        fields: [
          { id: 'siteUrl', field: { type: 'url' }, required: true },
          { id: 'token', field: { type: 'secret' }, required: true },
        ],
      }}
      values={{}}
      onChange={onChange}
    />)

    const token = screen.getByLabelText(/API token/)
    expect(token.getAttribute('type')).toBe('password')
    fireEvent.change(screen.getByLabelText(/Site URL/), { target: { value: 'https://acme.atlassian.net' }})
    expect(onChange).toHaveBeenCalledWith({ siteUrl: 'https://acme.atlassian.net' })
  })
})

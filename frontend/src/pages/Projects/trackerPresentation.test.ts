// Copyright © 2026 Jalapeno Labs

import type { ProjectTracker } from '../../api/routes/projectManagerRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeConnector } from '../../testFixtures'
import { draftCredential, toTrackerRequest, trackerDraftFrom, youtrackAccountOptions } from './trackerPresentation'

const ELY: ProjectTracker = {
  source: 'named',
  provider: 'jira',
  credentialId: 'work-jira',
  credentialName: 'Work',
  destination: 'ELY',
}

describe('trackerDraftFrom', () => {
  it('opens on the tracker the project has, or on Jira with nothing chosen', () => {
    expect(trackerDraftFrom(ELY)).toEqual({ provider: 'jira', credentialId: 'work-jira', destination: 'ELY' })
    expect(trackerDraftFrom(null)).toEqual({ provider: 'jira', credentialId: '', destination: '' })
  })

  it('leaves a GitHub tracker without a credential, since it files with the project\'s token', () => {
    const github: ProjectTracker = {
      ...ELY,
      provider: 'github',
      credentialId: 'the-projects-token',
      destination: 'acme/site',
    }
    expect(trackerDraftFrom(github).credentialId).toBe('')
  })
})

describe('draftCredential', () => {
  it('reads GitHub through the project\'s token and waits for an account elsewhere', () => {
    expect(draftCredential({ provider: 'github', credentialId: '', destination: '' })).toEqual({ credentialId: null })
    expect(draftCredential({ provider: 'youtrack', credentialId: '', destination: '' })).toBeNull()
    expect(draftCredential({ provider: 'youtrack', credentialId: 'mine', destination: '' }))
      .toEqual({ credentialId: 'mine' })
  })
})

describe('toTrackerRequest', () => {
  it('saves only a finished draft, naming a credential exactly where one is taken', () => {
    expect(toTrackerRequest({ provider: 'jira', credentialId: 'work-jira', destination: '' })).toBeNull()
    expect(toTrackerRequest({ provider: 'jira', credentialId: '', destination: 'ELY' })).toBeNull()
    expect(toTrackerRequest({ provider: 'jira', credentialId: 'work-jira', destination: 'ELY' }))
      .toEqual({ provider: 'jira', credentialId: 'work-jira', destination: 'ELY' })
    expect(toTrackerRequest({ provider: 'github', credentialId: 'ignored', destination: 'acme/site' }))
      .toEqual({ provider: 'github', destination: 'acme/site' })
  })
})

describe('youtrackAccountOptions', () => {
  it('lists the person\'s own YouTrack accounts once each', () => {
    const connectors = [
      makeConnector({ id: 'a', createdBy: 'ada', account: { id: 'ada-yt', kind: 'youtrack', name: 'Acme' }}),
      makeConnector({ id: 'b', createdBy: 'ada', account: { id: 'ada-yt', kind: 'youtrack', name: 'Acme' }}),
      makeConnector({ id: 'c', createdBy: 'grace', account: { id: 'grace-yt', kind: 'youtrack', name: 'Theirs' }}),
      makeConnector({ id: 'd', createdBy: 'ada', account: { id: 'ada-jira', kind: 'atlassian', name: 'Work' }}),
    ]
    expect(youtrackAccountOptions(connectors, 'ada')).toEqual([{ id: 'ada-yt', label: 'Acme' }])
    expect(youtrackAccountOptions(connectors, null)).toEqual([])
  })
})

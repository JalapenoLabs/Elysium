// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemComment, HistoryEntry } from './api/routes/actionItemRoutes'
import type { CodingSession } from './api/routes/codingSessionRoutes'
import type { Initiative } from './api/routes/initiativeRoutes'
import type { Project } from './api/routes/projectRoutes'

// Records shaped as the API sends them, for tests. Each starts from plain defaults and
// takes only the fields a test is about.

const CREATED_AT = '2026-09-01T00:00:00.000Z'

export function makeActionItem(overrides: Partial<ActionItem> & Pick<ActionItem, 'id'>): ActionItem {
  return {
    title: overrides.id,
    notes: '',
    state: 'open',
    priority: 'normal',
    dueAt: null,
    snoozedUntil: null,
    waitingOn: null,
    owner: { kind: 'user' },
    resolvedAt: null,
    dismissedAt: null,
    deletedAt: null,
    projectIds: [],
    initiativeIds: [],
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeInitiative(overrides: Partial<Initiative> & Pick<Initiative, 'id'>): Initiative {
  return {
    name: overrides.id,
    description: '',
    state: 'active',
    targetAt: null,
    projectIds: [],
    progress: { resolved: 0, total: 0 },
    deletedAt: null,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeComment(
  overrides: Partial<ActionItemComment> & Pick<ActionItemComment, 'id' | 'actionItemId'>,
): ActionItemComment {
  return {
    author: 'user',
    body: 'A comment',
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeHistoryEntry(overrides: Partial<HistoryEntry> & Pick<HistoryEntry, 'id'>): HistoryEntry {
  return {
    actionItemId: null,
    initiativeId: null,
    kind: 'created',
    actor: 'user',
    data: {},
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeCodingSession(overrides: Partial<CodingSession> & Pick<CodingSession, 'id'>): CodingSession {
  return {
    projectId: 'project',
    satelliteId: 'satellite',
    threadId: `thread-${overrides.id}`,
    title: `Session ${overrides.id}`,
    githubCredentialId: null,
    actionItemId: null,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    thread: null,
    ...overrides,
  }
}

export function makeProject(overrides: Partial<Project> & Pick<Project, 'id'>): Project {
  return {
    name: overrides.id,
    description: '',
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    coverUpdatedAt: null,
    coverFit: 'fit',
    github: { access: 'default', credentialId: null },
    ...overrides,
  }
}

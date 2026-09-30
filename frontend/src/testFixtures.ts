// Copyright © 2026 Jalapeno Labs

import type {
  ActionItem,
  ActionItemComment,
  ActionItemLink,
  HistoryEntry,
  PendingWrite,
} from './api/routes/actionItemRoutes'
import type { Changeset, ChangesetOperation } from './api/routes/changesetRoutes'
import type { CodingSession } from './api/routes/codingSessionRoutes'
import type { Initiative, InitiativeLink } from './api/routes/initiativeRoutes'
import type { Project } from './api/routes/projectRoutes'
import type { Satellite } from './api/routes/satelliteRoutes'
import type { StorageLocation } from './api/routes/storageRoutes'
import type { StudioAsset, StudioFeedback, StudioItem } from './api/routes/studioRoutes'

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
    changesetId: null,
    ...overrides,
  }
}

export function makeChangesetOperation(
  overrides: Partial<ChangesetOperation> & Pick<ChangesetOperation, 'id' | 'position' | 'operation'>,
): ChangesetOperation {
  return {
    reason: 'Sam asked for it',
    quote: null,
    source: null,
    dependsOn: [],
    decision: 'pending',
    outcome: 'pending',
    error: null,
    result: {},
    undo: null,
    ...overrides,
  }
}

export function makeChangeset(overrides: Partial<Changeset> & Pick<Changeset, 'id'>): Changeset {
  return {
    proposer: 'elysia',
    projectId: null,
    summary: 'Follow-ups from the planning meeting',
    state: 'pending',
    decidedAt: null,
    undoneAt: null,
    operations: [],
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
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
    studioItemId: null,
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

export function makeActionItemLink(
  overrides: Partial<ActionItemLink> & Pick<ActionItemLink, 'id' | 'actionItemId'>,
): ActionItemLink {
  return {
    provider: 'jira',
    kind: 'issue',
    credentialId: 'credential',
    key: 'ELY-1',
    url: 'https://acme.atlassian.net/browse/ELY-1',
    title: 'An issue',
    isPrimary: false,
    state: 'open',
    owner: { kind: 'user' },
    pendingWrites: [],
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makePendingWrite(overrides: Partial<PendingWrite> & Pick<PendingWrite, 'id'>): PendingWrite {
  return {
    kind: 'close',
    commentId: null,
    attempts: 0,
    lastError: null,
    lastAttemptAt: null,
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeInitiativeLink(
  overrides: Partial<InitiativeLink> & Pick<InitiativeLink, 'id' | 'initiativeId'>,
): InitiativeLink {
  return {
    provider: 'github',
    kind: 'milestone',
    credentialId: 'credential',
    key: 'acme/elysium#3',
    url: 'https://github.com/acme/elysium/milestone/3',
    title: 'A milestone',
    syncedAt: null,
    syncError: null,
    truncated: false,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeStudioItem(overrides: Partial<StudioItem> & Pick<StudioItem, 'id'>): StudioItem {
  return {
    title: overrides.id,
    prompt: 'Model a banana',
    projectId: null,
    storageLocationId: 'location',
    thumbnailAssetId: null,
    pinnedAssetId: null,
    imageCount: 0,
    modelCount: 0,
    pullError: null,
    deletedAt: null,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeStudioAsset(
  overrides: Partial<StudioAsset> & Pick<StudioAsset, 'id' | 'artifactPath'>,
): StudioAsset {
  return {
    studioItemId: 'item',
    sessionId: 1,
    kind: 'image',
    name: overrides.artifactPath.split('/').at(-1) ?? overrides.artifactPath,
    contentType: null,
    sizeBytes: 1024,
    sha256: `sha-${overrides.id}`,
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeStudioFeedback(overrides: Partial<StudioFeedback> & Pick<StudioFeedback, 'id'>): StudioFeedback {
  return {
    studioItemId: 'item',
    sessionId: 1,
    turnId: null,
    prompt: 'Make it yellower',
    sourceAssetId: null,
    cameraOrbit: null,
    hasCapture: true,
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeStorageLocation(
  overrides: Partial<StorageLocation> & Pick<StorageLocation, 'id'>,
): StorageLocation {
  return {
    name: overrides.id,
    provider: { kind: 'bunny', zone: 'zone', region: 'frankfurt' },
    pathPrefix: '',
    storageLimitBytes: null,
    projects: '*',
    isStudioDefault: false,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeSatellite(overrides: Partial<Satellite> & Pick<Satellite, 'id'>): Satellite {
  return {
    name: overrides.id,
    description: '',
    url: 'http://satellite.test',
    isActive: true,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    status: {
      satelliteId: overrides.id,
      reachable: true,
      version: '1.0.0',
      runningThreads: 0,
      maxConcurrentThreads: 4,
      error: null,
      setup: { state: 'succeeded', isCurrent: true, failureOutput: null },
    },
    ...overrides,
  }
}

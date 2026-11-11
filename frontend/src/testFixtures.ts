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
import type { Connector } from './api/routes/connectorRoutes'
import type { Draft } from './api/routes/draftRoutes'
import type { Finding } from './api/routes/findingRoutes'
import type { MailAccount } from './api/routes/mailRoutes'
import type { Initiative, InitiativeLink } from './api/routes/initiativeRoutes'
import type { LearningCategory } from './api/routes/learningCategoryRoutes'
import type { Learning, LearningHistoryEntry } from './api/routes/learningRoutes'
import type { KnowledgePage, KnowledgePageSummary, PageHistoryEntry, PageVersion } from './api/routes/pageRoutes'
import type { ClaudeAccount, Llm } from './api/routes/llmRoutes'
import type { Meeting } from './api/routes/meetingRoutes'
import type { ProjectManager, WorkBatch } from './api/routes/projectManagerRoutes'
import type { Project } from './api/routes/projectRoutes'
import type { PersonPromise } from './api/routes/promiseRoutes'
import type { RunStep, WorkflowRun } from './api/routes/runRoutes'
import type { StorageLocation } from './api/routes/storageRoutes'
import type { StudioAsset, StudioFeedback, StudioItem } from './api/routes/studioRoutes'
import type { TimelineRecord } from './api/routes/timelineRoutes'
import type { User } from './api/routes/userRoutes'

// Records shaped as the API sends them, for tests. Each starts from plain defaults and
// takes only the fields a test is about.

const CREATED_AT = '2026-09-01T00:00:00.000Z'

// The system user, which owns what predates accounts.
const CREATED_BY = '00000000-0000-0000-0000-000000000001'

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
    projectAttributions: [],
    leadProjectId: null,
    managerAssignment: null,
    initiativeIds: [],
    createdAt: CREATED_AT,
    createdBy: CREATED_BY,
    updatedAt: CREATED_AT,
    issueFiling: null,
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
    createdBy: CREATED_BY,
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
    causeRecordId: null,
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
    createdBy: '00000000-0000-0000-0000-000000000002',
    decidedBy: null,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeCodingSession(overrides: Partial<CodingSession> & Pick<CodingSession, 'id'>): CodingSession {
  return {
    projectId: 'project',
    title: `Session ${overrides.id}`,
    githubCredentialId: null,
    actionItemId: null,
    studioItemId: null,
    createdAt: CREATED_AT,
    createdBy: CREATED_BY,
    updatedAt: CREATED_AT,
    activity: { state: 'idle', queueDepth: 0, currentTurnId: null, lastActivityAt: null },
    ...overrides,
  }
}

export function makeProject(overrides: Partial<Project> & Pick<Project, 'id'>): Project {
  return {
    name: overrides.id,
    description: '',
    createdAt: CREATED_AT,
    createdBy: CREATED_BY,
    updatedAt: CREATED_AT,
    coverUpdatedAt: null,
    coverFit: 'fit',
    github: { access: 'default', credentialId: null },
    autonomy: 'approve_every_batch',
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
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeLearning(overrides: Partial<Learning> & Pick<Learning, 'id'>): Learning {
  return {
    title: overrides.id,
    body: 'Run REINDEX DATABASE.',
    category: 'procedure',
    evidence: 'Lookups missed rows after the image changed.',
    agent: null,
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    deletedAt: null,
    ...overrides,
  }
}

export function makePageSummary(
  overrides: Partial<KnowledgePageSummary> & Pick<KnowledgePageSummary, 'id'>,
): KnowledgePageSummary {
  return {
    parentId: null,
    position: 0,
    title: 'A page',
    currentVersion: 1,
    fixed: false,
    briefProjectId: null,
    labels: [],
    pendingSuggestions: 0,
    updatedAt: '2026-11-01T12:00:00Z',
    deletedAt: null,
    ...overrides,
  }
}

export function makePage(overrides: Partial<KnowledgePage> & Pick<KnowledgePage, 'id'>): KnowledgePage {
  return {
    projectIds: [],
    ...makePageSummary(overrides),
    body: '',
    attachments: [],
    category: null,
    evidence: null,
    proposalOnly: false,
    companyDoc: false,
    publishedBy: 'user-1',
    publishedAt: '2026-11-01T12:00:00Z',
    agent: null,
    createdBy: 'user-1',
    createdAt: '2026-11-01T12:00:00Z',
    ...overrides,
  }
}

export function makePageHistoryEntry(
  overrides: Partial<PageHistoryEntry> & Pick<PageHistoryEntry, 'id'>,
): PageHistoryEntry {
  return {
    pageId: 'page-1',
    kind: 'published',
    createdBy: 'user-1',
    agent: null,
    data: {},
    createdAt: '2026-11-01T12:00:00Z',
    ...overrides,
  }
}

export function makePageVersion(
  overrides: Partial<PageVersion> & Pick<PageVersion, 'pageId' | 'version'>,
): PageVersion {
  return {
    title: 'A page',
    restoredFrom: null,
    createdBy: 'user-1',
    agent: null,
    createdAt: '2026-11-01T12:00:00Z',
    body: '',
    category: null,
    evidence: null,
    ...overrides,
  }
}

export function makeLearningCategory(
  overrides: Partial<LearningCategory> & Pick<LearningCategory, 'key'>,
): LearningCategory {
  return {
    label: overrides.key,
    description: `Everything about ${overrides.key}.`,
    position: 0,
    builtIn: false,
    learningCount: 0,
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeLearningHistoryEntry(
  overrides: Partial<LearningHistoryEntry> & Pick<LearningHistoryEntry, 'id'>,
): LearningHistoryEntry {
  return {
    learningId: 'learning',
    kind: 'created',
    createdBy: CREATED_BY,
    agent: null,
    data: {},
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeUser(overrides: Partial<User> = {}): User {
  return {
    id: 'person',
    kind: 'person',
    name: 'Ada Lovelace',
    email: 'ada@example.com',
    role: 'member',
    status: 'active',
    approvedAt: CREATED_AT,
    lastSeenAt: null,
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
    createdBy: CREATED_BY,
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
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

// A Claude account signed in, with no usage read yet.
export function makeClaudeAccount(overrides: Partial<ClaudeAccount> = {}): ClaudeAccount {
  return {
    email: 'alex@example.com',
    organizationName: null,
    subscriptionType: 'max',
    isSignedIn: true,
    accessTokenExpiresAt: '2026-10-21T08:00:00Z',
    lastRefreshedAt: null,
    lastRefreshError: null,
    usage: { fiveHour: null, sevenDay: null, readAt: null },
    usageError: null,
    ...overrides,
  }
}

export function makeLlm(overrides: Partial<Llm> & Pick<Llm, 'id'>): Llm {
  return {
    name: overrides.id,
    description: '',
    type: 'claude-api-token',
    priority: 0,
    isActive: true,
    expiresAt: null,
    createdAt: CREATED_AT,
    createdBy: CREATED_BY,
    updatedAt: CREATED_AT,
    account: null,
    ...overrides,
  }
}

// A Jira connector reading well, on its Atlassian account.
export function makeConnector(overrides: Partial<Connector> & Pick<Connector, 'id'>): Connector {
  return {
    kind: 'jira',
    scope: 'workspace',
    account: { id: 'account-1', kind: 'atlassian', name: 'Work' },
    status: 'active',
    isActive: true,
    signInState: 'unknown',
    lastSyncAt: null,
    lastError: null,
    records: 0,
    content: {},
    storageLocationId: null,
    checks: [],
    lastRun: null,
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeTimelineRecord(
  overrides: Partial<TimelineRecord> & Pick<TimelineRecord, 'id'>,
): TimelineRecord {
  return {
    sourceKind: 'slack',
    connectorId: 'connector-1',
    externalId: 'T1/C1/1730000000.000200',
    occurredAt: CREATED_AT,
    actor: 'Ada',
    title: '#launch in Mooreslab',
    body: 'Are we launching Friday?',
    url: null,
    media: null,
    metadata: {},
    actionItemId: null,
    projects: [],
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeDraft(overrides: Partial<Draft> & Pick<Draft, 'id'>): Draft {
  return {
    replyToRecordId: null,
    connectorId: 'connector-1',
    mailAccountId: null,
    target: {},
    place: 'thread',
    body: 'Friday works.',
    state: 'draft',
    lastError: null,
    attemptedAt: null,
    sentAt: null,
    sentRecordId: null,
    promiseId: null,
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeMailAccount(overrides: Partial<MailAccount> & Pick<MailAccount, 'id'>): MailAccount {
  return {
    kind: 'gmail',
    address: 'alex@example.com',
    displayName: 'Alex',
    isActive: true,
    lastCheckedAt: null,
    lastError: null,
    createdAt: CREATED_AT,
    createdBy: CREATED_BY,
    updatedAt: CREATED_AT,
    mailDomainId: null,
    ...overrides,
  }
}

export function makeRun(overrides: Partial<WorkflowRun> & Pick<WorkflowRun, 'id'>): WorkflowRun {
  return {
    workflow: 'meeting',
    sourceKind: 'meeting',
    sourceId: 'm-1',
    state: 'running',
    error: null,
    createdBy: CREATED_BY,
    createdAt: CREATED_AT,
    startedAt: CREATED_AT,
    finishedAt: null,
    ...overrides,
  }
}

export function makeMeeting(overrides: Partial<Meeting> & Pick<Meeting, 'id'>): Meeting {
  return {
    connectorId: 'connector-1',
    sourceKind: 'fireflies',
    title: 'Weekly planning',
    startedAt: CREATED_AT,
    durationMs: 30 * 60 * 1000,
    organizer: 'sam@example.com',
    participants: [],
    url: null,
    state: 'done',
    lastError: null,
    hasRecording: true,
    segmentCount: 3,
    holdForApproval: false,
    correction: null,
    runId: null,
    awaitingApproval: false,
    mergedInto: null,
    createdAt: CREATED_AT,
    updatedAt: CREATED_AT,
    ...overrides,
  }
}

export function makeRunStep(overrides: Partial<RunStep> & Pick<RunStep, 'id' | 'runId'>): RunStep {
  return {
    step: 'fetch_transcript',
    state: 'pending',
    summary: null,
    error: null,
    attempts: 0,
    availableAt: null,
    startedAt: null,
    finishedAt: null,
    ...overrides,
  }
}

export function makeWorkBatch(overrides: Partial<WorkBatch> & Pick<WorkBatch, 'id'>): WorkBatch {
  return {
    projectId: 'project-1',
    title: 'Pricing page',
    goal: 'Show annual pricing on the pricing page.',
    serves: 'Annual pricing',
    repositories: [{ url: 'https://github.com/JalapenoLabs/Elysium.git' }],
    priority: 'normal',
    scheduledFor: null,
    state: 'proposed',
    result: null,
    turnId: null,
    sourceRecordIds: [],
    actionItemIds: [],
    sourceMessageIds: [],
    origin: 'human_rooted',
    generation: 0,
    parentBatchIds: [],
    timelineRecordId: null,
    pinnedAt: null,
    assignedAt: null,
    createdAt: CREATED_AT,
    startedAt: null,
    finishedAt: null,
    waitingOn: null,
    waitingDetail: null,
    waitingUntil: null,
    configuration: null,
    ...overrides,
  }
}

export function makeFinding(overrides: Partial<Finding> & Pick<Finding, 'id'>): Finding {
  return {
    projectId: 'project-1',
    title: 'Retry loop never gives up',
    detail: 'The retry loop never gives up when the provider answers 429.',
    repositoryUrl: 'https://github.com/JalapenoLabs/Elysium.git',
    location: 'api/src/retry.rs',
    cost: 'small',
    state: 'open',
    workBatchId: 'batch-1',
    projectManagerTurnId: null,
    reports: 1,
    lastReportedAt: CREATED_AT,
    decidedBy: null,
    decidedAt: null,
    dismissalReason: null,
    actionItemId: null,
    acceptedBatchId: null,
    createdBy: 'coding-agent',
    createdAt: CREATED_AT,
    ...overrides,
  }
}

export function makeProjectManager(
  overrides: Partial<ProjectManager> & Pick<ProjectManager, 'projectId'>,
): ProjectManager {
  return {
    status: 'idle',
    waiting: 0,
    usagePausedUntil: null,
    configurationHold: null,
    turns: [],
    batches: [],
    proposed: [],
    promises: [],
    writes: { direct: false, tracker: null, trackerRefusal: null },
    ...overrides,
  }
}

export function makePromise(overrides: Partial<PersonPromise> & Pick<PersonPromise, 'id'>): PersonPromise {
  return {
    projectId: 'project-1',
    state: 'waiting',
    recipient: 'Ada',
    summary: 'when the login fix is live',
    madeIn: {
      recordId: 'record-1',
      sourceKind: 'slack',
      title: 'Reply in #launch in Mooreslab',
      url: 'https://mooreslab.slack.com/archives/C1/p1730000000000200',
      occurredAt: CREATED_AT,
      personId: CREATED_BY,
    },
    workBatchId: null,
    workItemId: null,
    dueAt: null,
    followUpItemId: null,
    draftId: null,
    createdAt: CREATED_AT,
    ...overrides,
  }
}

// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemComment, ActionItemLink, HistoryEntry } from '../api/routes/actionItemRoutes'
import type { BackupsStatus } from '../api/routes/backupRoutes'
import type { BrowserSession } from '../api/routes/browserSessionRoutes'
import type { Changeset } from '../api/routes/changesetRoutes'
import type { Initiative, InitiativeLink } from '../api/routes/initiativeRoutes'
import type { CodingSession, SessionEvent, TurnSummary } from '../api/routes/codingSessionRoutes'
import type { Elysia, ElysiaMessage, ElysiaTurn } from '../api/routes/elysiaRoutes'
import type { EnvironmentVariable } from '../api/routes/environmentRoutes'
import type { FindingState } from '../api/routes/findingRoutes'
import type { GithubCredential } from '../api/routes/githubRoutes'
import type { JiraCredential } from '../api/routes/jiraRoutes'
import type { KnowledgeAskState } from '../api/routes/knowledgeRoutes'
import type { Llm, LlmUsage } from '../api/routes/llmRoutes'
import type { MailAccount, MailDomain, MailServer } from '../api/routes/mailRoutes'
import type { MeetingState } from '../api/routes/meetingRoutes'
import type { AgentKind } from '../api/routes/modelSettingRoutes'
import type { PasswordVault, VaultStatus } from '../api/routes/passwordVaultRoutes'
import type { Project } from '../api/routes/projectRoutes'
import type { WorkBatchState } from '../api/routes/projectManagerRoutes'
import type { RunState, StepState } from '../api/routes/runRoutes'
import type { StorageLocation } from '../api/routes/storageRoutes'
import type { StudioAsset, StudioFeedback, StudioItem } from '../api/routes/studioRoutes'
import type { UpdatesStatus } from '../api/routes/updateRoutes'
import type { User, WorkspaceSettings } from '../api/routes/userRoutes'

// Everything the API's event stream can send. Mirrors `ServerEvent` in
// api/src/realtime.rs: each SSE message's data is one of these as JSON.
export type ServerEvent =
  // First message on every connection; load state after it.
  | { type: 'hello' }
  // Events were missed; refetch everything shown.
  | { type: 'resync' }
  // Also sent when an item joins or leaves a project or initiative, and when it is restored.
  | { type: 'actionItem.upserted', data: ActionItem }
  // Soft: the item can be restored.
  | { type: 'actionItem.deleted', data: { id: string } }
  // Sent for every link of an item whenever one of them, or a write it owes, changes.
  | { type: 'actionItemLink.upserted', data: ActionItemLink }
  | { type: 'actionItemLink.deleted', data: { id: string, actionItemId: string } }
  // A container was linked, or the watcher read it.
  | { type: 'initiativeLink.upserted', data: InitiativeLink }
  | { type: 'initiativeLink.deleted', data: { id: string, initiativeId: string } }
  | { type: 'actionItemComment.upserted', data: ActionItemComment }
  | { type: 'actionItemComment.deleted', data: { id: string, actionItemId: string } }
  // Also sent whenever the initiative's progress moves.
  | { type: 'initiative.upserted', data: Initiative }
  // Soft: the initiative can be restored.
  | { type: 'initiative.deleted', data: { id: string } }
  // A browser was added, or began starting, running, or failing.
  | { type: 'browserSession.upserted', data: BrowserSession }
  | { type: 'browserSession.deleted', data: { id: string } }
  // A browser went back to its start page, its other tabs closed and its profile kept.
  | { type: 'browserSession.startedOver', data: { id: string, createdBy: string } }
  // A connector changed; its id alone, since its view names whose mail it reads.
  | { type: 'connector.changed', data: { id: string } }
  | { type: 'connector.deleted', data: { id: string } }
  // A learning changed; its id alone, since a guest hears every event and reaches no
  // knowledge base.
  | { type: 'learning.changed', data: { id: string } }
  // A knowledge base page changed, once for each page a change touched; its id alone, for the
  // same reason.
  | { type: 'page.changed', data: { id: string } }
  // One of someone's questions to the knowledge base was asked, taken, answered, or failed. Its
  // id, asker, and state alone: the question and its answer are the asker's, who loads it.
  | { type: 'knowledgeAsk.changed', data: { id: string, askedBy: string, state: KnowledgeAskState } }
  // A workspace learning category was added, edited, reordered, or removed. Nothing more, for
  // the same reason; members load the list again.
  | { type: 'learningCategories.changed' }
  // An admin chose where page attachments go. Nothing more, for the same reason; members load it
  // again.
  | { type: 'pageAttachments.changed' }
  // An admin chose the largest file a meeting takes. Nothing more, for the same reason; everyone
  // loads the policy again.
  | { type: 'meetingAttachments.changed' }
  // An admin saved or restored a version of the agent guidance. Nothing more; members load it
  // again.
  | { type: 'agentGuidance.changed' }
  // An admin chose what a step or agent runs on, or let it follow its kind again. Its name alone;
  // admins load the list again.
  | { type: 'modelSetting.changed', data: { agent: string } }
  // Someone chose what a kind of agent runs on, for the workspace (`projectId` null) or for one
  // project, or put it back. Pages showing either load it again.
  | { type: 'kindModelSetting.changed', data: { kind: AgentKind, projectId: string | null } }
  // A check in a run moved on, or (`checkId` null, `state` finished) the run ended.
  | {
    type: 'connectorCheckRun.updated'
    data: {
      runId: string
      connectorId: string | null
      checkId: string | null
      state: 'queued' | 'running' | 'passed' | 'failed' | 'skipped' | 'finished'
    }
  }
  // Something was ingested onto someone's timeline. Never its content, which its owner loads.
  | {
    type: 'timelineRecord.created',
    data: {
      id: string
      sourceKind: string
      createdBy: string
      actionItemId: string | null
      // The conversation it joined, when it is a message work starts from.
      conversationId: string | null
    },
  }
  // What a record is attributed to changed. Its id and owner alone; its owner loads it.
  | { type: 'timelineRecord.changed', data: { id: string, createdBy: string } }
  // A draft was written, changed, discarded, or sent, or its send failed. Its id and writer
  // alone, since its text is theirs.
  | { type: 'draft.changed', data: { id: string, createdBy: string } }
  // A meeting was read, its processing moved on, or someone changed it, who sees it included. Its
  // id alone: every client loads it, and one that may not read it is answered 404.
  | { type: 'meeting.changed', data: { id: string, state: MeetingState, entryId: string | null } }
  // An entry an item's or an initiative's write recorded.
  | { type: 'history.appended', data: HistoryEntry }
  // A changeset was proposed, decided on, applied, or undone.
  | { type: 'changeset.upserted', data: Changeset }
  // Elysia's status, settings, open questions, or work changed; carries all of it.
  | { type: 'elysia.updated', data: Elysia }
  | { type: 'elysiaMessage.created', data: ElysiaMessage }
  // One of her turns was queued, started, ended, or cancelled.
  | { type: 'elysiaTurn.upserted', data: ElysiaTurn }
  // One of her turns that read a meeting started or ended. Its id alone, since what it says is
  // read only by that meeting's readers; each client loads it, and one that may not is told 404.
  | { type: 'elysiaTurn.changed', data: { id: string } }
  // A project's Project Manager changed: something woke it, a turn started or ended, or a batch
  // of its was submitted or moved on. The project's id alone; its page loads it.
  | { type: 'projectManager.changed', data: { projectId: string } }
  | { type: 'workBatch.changed', data: { id: string, projectId: string, state: WorkBatchState } }
  | { type: 'workerPool.changed' }
  // A finding was reported, reported again, accepted, or dismissed. Its ids and state alone,
  // since what it says quotes agents' work; its project's Findings tab loads it.
  | { type: 'finding.changed', data: { id: string, projectId: string, state: FindingState } }
  // A project's worker setup was replaced: its tools script or the repositories it declares.
  // The project's id alone; its Workers page loads the setup again.
  | { type: 'projectWorkerSetup.changed', data: { projectId: string } }
  | { type: 'environmentVariable.upserted', data: EnvironmentVariable }
  | { type: 'environmentVariable.deleted', data: { id: string } }
  | { type: 'githubCredential.upserted', data: GithubCredential }
  | { type: 'githubCredential.deleted', data: { id: string } }
  | { type: 'jiraCredential.upserted', data: JiraCredential }
  | { type: 'jiraCredential.deleted', data: { id: string } }
  | { type: 'llm.upserted', data: Llm }
  | { type: 'llm.deleted', data: { id: string } }
  // A Claude account's usage changed. Numbers and instants only.
  | { type: 'llm.usage', data: { id: string, usage: LlmUsage } }
  | { type: 'mailbox.upserted', data: MailAccount }
  | { type: 'mailbox.deleted', data: { id: string } }
  // Also sent for each step while the server is created.
  | { type: 'mailServer.updated', data: MailServer }
  | { type: 'mailDomain.upserted', data: MailDomain }
  | { type: 'mailDomain.deleted', data: { id: string } }
  | { type: 'project.upserted', data: Project }
  | { type: 'project.deleted', data: { id: string } }
  | { type: 'storageLocation.upserted', data: StorageLocation }
  | { type: 'storageLocation.deleted', data: { id: string } }
  | { type: 'session.upserted', data: CodingSession }
  | { type: 'session.deleted', data: { id: number } }
  | { type: 'session.event', data: SessionEvent }
  | { type: 'session.summary', data: TurnSummary }
  // The summary of a turn that read a meeting was written. Its ids alone, since it is read only
  // by that meeting's readers; a conversation showing the session loads its summaries again.
  | { type: 'session.summaryChanged', data: { sessionId: number, turnId: string } }
  // Created, renamed, pinned, or restored, or a file it holds changed its counts or thumbnail.
  | { type: 'studioItem.upserted', data: StudioItem }
  // Soft or permanent; the event does not say which.
  | { type: 'studioItem.deleted', data: { id: string } }
  // A file was pulled from a workspace and stored; every version is its own asset.
  | { type: 'studioAsset.created', data: StudioAsset }
  // A prompt with a drawing was sent.
  | { type: 'studioFeedback.created', data: StudioFeedback }
  // A database backup began or ended, or an admin chose where backups go.
  | { type: 'backups.status', data: BackupsStatus }
  // Auto-update's state changed: a check, an update beginning or ending, or what it waits for.
  | { type: 'updates.status', data: UpdatesStatus }
  // Someone signed up, was approved, changed role, or was disabled or enabled.
  | { type: 'user.upserted', data: User }
  // A pending sign-up was rejected.
  | { type: 'user.deleted', data: { id: string } }
  | { type: 'workspaceSettings.updated', data: WorkspaceSettings }
  // The password vault was set up, its collection chosen, or its credentials replaced.
  | { type: 'passwordVault.updated', data: PasswordVault }
  | { type: 'passwordVault.deleted' }
  // What the vault's CLI is doing changed: starting, ready, failed, or a sync finished.
  | { type: 'passwordVault.status', data: VaultStatus }
  // A run started, it or one of its steps moved on, or a person acted on a step. Its id and
  // state alone, since a run reads what was ingested; members load it by id.
  | { type: 'workflowRun.changed', data: { id: string, state: RunState } }
  // A run's step moved on, or wrote to its log.
  | { type: 'stepJob.changed', data: { id: string, runId: string, state: StepState } }

export type ServerEventType = ServerEvent['type']

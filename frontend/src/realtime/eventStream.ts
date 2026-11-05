// Copyright © 2026 Jalapeno Labs

import type { ServerEvent, ServerEventType } from './serverEvents'

// Core
import { mutate } from 'swr'

// Redux
import { store } from '../store'
import { selectIsAdmin, selectIsCompanyMember } from '../store/authSlice'
import { actionItemCommentDeleted, actionItemCommentUpserted } from '../store/actionItemCommentsSlice'
import { historyAppended } from '../store/actionItemHistorySlice'
import { actionItemLinkDeleted, actionItemLinkUpserted } from '../store/actionItemLinksSlice'
import { initiativeLinkDeleted, initiativeLinkUpserted } from '../store/initiativeLinksSlice'
import { actionItemDeleted, actionItemUpserted } from '../store/actionItemsSlice'
import { browserSessionDeleted, browserSessionUpserted } from '../store/browserSessionsSlice'
import { changesetUpserted } from '../store/changesetsSlice'
import {
  checkRunLoaded,
  checkRunProgressed,
  connectorDeleted,
  connectorUpserted,
} from '../store/connectorsSlice'
import { codingSessionDeleted, codingSessionUpserted } from '../store/codingSessionsSlice'
import { elysiaMessageCreated, elysiaTurnUpserted, elysiaUpdated } from '../store/elysiaSlice'
import { environmentVariableDeleted, environmentVariableUpserted } from '../store/environmentVariablesSlice'
import { githubCredentialDeleted, githubCredentialUpserted } from '../store/githubCredentialsSlice'
import { initiativeDeleted, initiativeUpserted } from '../store/initiativesSlice'
import { jiraCredentialDeleted, jiraCredentialUpserted } from '../store/jiraCredentialsSlice'
import { knowledgeAskUpserted } from '../store/knowledgeAsksSlice'
import { learningRemoved, learningUpserted } from '../store/learningsSlice'
import { llmDeleted, llmUpserted, llmUsageUpdated } from '../store/llmsSlice'
import { mailAccountDeleted, mailAccountUpserted } from '../store/mailAccountsSlice'
import { mailDomainDeleted, mailDomainUpserted } from '../store/mailDomainsSlice'
import { mailServerUpdated } from '../store/mailServerSlice'
import { draftRemoved, draftUpserted } from '../store/draftsSlice'
import { meetingUpserted, selectAllMeetings } from '../store/meetingsSlice'
import {
  passwordVaultDeleted,
  passwordVaultStatusUpdated,
  passwordVaultUpdated,
} from '../store/passwordVaultSlice'
import { projectDeleted, projectUpserted } from '../store/projectsSlice'
import { backupsStatusUpdated } from '../store/backupsSlice'
import { eventStreamLost, eventStreamOpened, eventStreamStopped } from '../store/realtimeSlice'
import { runLoaded, selectRunById, selectStepLog, stepLogLoaded } from '../store/runsSlice'
import { sessionEventReceived, turnSummaryReceived } from '../store/sessionEventsSlice'
import { storageLocationDeleted, storageLocationUpserted } from '../store/storageLocationsSlice'
import { studioAssetCreated } from '../store/studioAssetsSlice'
import { studioFeedbackCreated } from '../store/studioFeedbackSlice'
import { studioItemDeleted, studioItemUpserted } from '../store/studioItemsSlice'
import { timelineRecordUpserted } from '../store/timelineRecordsSlice'
import { userDeleted, userUpserted } from '../store/usersSlice'
import { updatesStatusUpdated } from '../store/updatesSlice'
import { buildRead } from '../store/versionSlice'
import { workspaceSettingsUpdated } from '../store/workspaceSettingsSlice'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getCheckRun, getConnector } from '../api/routes/connectorRoutes'
import { getDraft } from '../api/routes/draftRoutes'
import { getVersion } from '../api/routes/healthRoutes'
import { AGENT_GUIDANCE_KEY } from '../api/routes/agentGuidanceRoutes'
import { COMPANY_DOC_HISTORY_KEY, COMPANY_DOC_KEY } from '../api/routes/companyDocRoutes'
import { getKnowledgeAsk } from '../api/routes/knowledgeRoutes'
import { LEARNING_CATEGORIES_KEY } from '../api/routes/learningCategoryRoutes'
import { getProjectModelSettingsKey, MODEL_SETTINGS_KEY } from '../api/routes/modelSettingRoutes'
import { getLearning, LEARNING_FEED_KEY, learningHistoryKey } from '../api/routes/learningRoutes'
import { getMeeting } from '../api/routes/meetingRoutes'
import { reloadChangedPage } from './pageChanges'
import { getProjectManagerKey } from '../api/routes/projectManagerRoutes'
import { PROMISES_KEY_PREFIX } from '../api/routes/promiseRoutes'
import {
  getWorkBatchKey,
  getWorkBatchPullRequestsKey,
  getWorkerLogKey,
  WORK_QUEUE_KEY,
} from '../api/routes/workQueueRoutes'
import { getProjectWorkerSetupKey } from '../api/routes/workerSetupRoutes'
import { getRun, getStepLog } from '../api/routes/runRoutes'
import { getTimelineRecord } from '../api/routes/timelineRoutes'
import {
  EVENT_STREAM_PATH,
  EVENT_STREAM_RETRY_INITIAL_MS,
  EVENT_STREAM_RETRY_MAX_MS,
  WORKSPACE_RECORD_OWNERS,
} from '../constants'

// The universal event stream: one EventSource for the whole app, open for as long as
// someone may use the workspace. Every change the API announces lands in Redux through here,
// so no component polls or refetches after another tab's write. `AuthGate` starts it once
// the signed-in person has access, and signing out stops it.

type Handlers = {
  [Type in ServerEventType]: (event: Extract<ServerEvent, { type: Type }>) => void
}

// Refetches everything on screen: every SWR key a mounted component holds, whose
// loaders put the responses in Redux (`src/hooks/useServerData.ts`). Data no component
// shows is refetched when one mounts.
function reloadEverything() {
  void mutate(() => true)
}

// Reads which build the API runs. The first answer is the build this page loaded; a later
// one that differs offers the person a refresh (`RefreshPrompt`). A new build only ever runs
// in a new API process, which drops every event stream, so the reconnection's `hello` is when
// to look again: no clock asks, and nothing is read while nothing changed.
function readRunningBuild() {
  getVersion()
    .then((version) => store.dispatch(buildRead(version.builtAt)))
    .catch((error: unknown) => console.debug('The running build could not be read', { error }))
}

// Whether the next `hello` reloads what is on screen. Events sent while no stream listened are
// gone for good, so it does after any failure: a dropped stream missed what was sent while it
// was down, and one that failed before it first opened let `AuthGate` show the workspace
// unsubscribed. A run's first connection needs no reload: `AuthGate` holds the workspace back
// until the stream opens, and the server subscribes before it answers, so everything on screen
// was read after the subscription.
let mustReloadOnHello = true

const handlers: Handlers = {
  'hello': () => {
    if (mustReloadOnHello) {
      reloadEverything()
      readRunningBuild()
    }
    mustReloadOnHello = true
  },
  'resync': () => reloadEverything(),
  'actionItem.upserted': (event) => store.dispatch(actionItemUpserted(event.data)),
  // The event carries only the id, so the deleted list and the item's own page reload, each
  // only if a view shows it; the page then shows the item as deleted rather than missing.
  'actionItem.deleted': (event) => {
    store.dispatch(actionItemDeleted(event.data.id))
    void mutate('v1/action-items?deleted=true')
    void mutate(`v1/action-items/${event.data.id}`)
  },
  // What the provider says about the item's links is read live, so a changed link rereads
  // it, if a page shows it.
  'actionItemLink.upserted': (event) => {
    store.dispatch(actionItemLinkUpserted(event.data))
    void mutate(`v1/action-items/${event.data.actionItemId}/links/remote`)
  },
  'actionItemLink.deleted': (event) => store.dispatch(actionItemLinkDeleted(event.data.id)),
  'initiativeLink.upserted': (event) => store.dispatch(initiativeLinkUpserted(event.data)),
  'initiativeLink.deleted': (event) => store.dispatch(initiativeLinkDeleted(event.data.id)),
  'actionItemComment.upserted': (event) => store.dispatch(actionItemCommentUpserted(event.data)),
  'actionItemComment.deleted': (event) => store.dispatch(actionItemCommentDeleted(event.data.id)),
  // Progress moved or the initiative changed; its burnup, if a page draws it, reloads.
  'initiative.upserted': (event) => {
    store.dispatch(initiativeUpserted(event.data))
    void mutate(`v1/initiatives/${event.data.id}/progress`)
  },
  // Reloads the same way as a deleted item.
  'initiative.deleted': (event) => {
    store.dispatch(initiativeDeleted(event.data.id))
    void mutate('v1/initiatives?deleted=true')
    void mutate(`v1/initiatives/${event.data.id}`)
  },
  // Every client hears every browser's events, but a browser belongs to its owner alone, and
  // only the owner's pages list it.
  'browserSession.upserted': (event) => {
    if (event.data.createdBy !== store.getState().auth.me?.user.id) {
      return
    }
    store.dispatch(browserSessionUpserted(event.data))
  },
  'browserSession.deleted': (event) => store.dispatch(browserSessionDeleted(event.data.id)),
  // Nothing Redux holds changes: the browser's status stays as it was, and its stream shows
  // the new page itself.
  'browserSession.startedOver': () => {},
  // The event carries the id alone, since a connector's view names whose mail it reads, so it
  // is loaded by id; someone else's personal connector answers 404 and is not kept. The hub's
  // activity feed and accounts list reload with it, if a page shows them.
  'connector.changed': (event) => {
    getConnector(event.data.id)
      .then((response) => store.dispatch(connectorUpserted(response.connector)))
      .catch((error: unknown) => {
        if (error instanceof HTTPError && error.response.status === 404) {
          store.dispatch(connectorDeleted(event.data.id))
          return
        }
        console.debug('A changed connector could not be loaded', {
          error,
          connectorId: event.data.id,
        })
      })
    void mutate('v1/connectors/activity')
    void mutate('v1/accounts')
  },
  // The event carries the id alone, so the learning is loaded by id, by the company's members
  // only: a guest's request would be refused. Its history, the admins' feed, and the categories,
  // whose counts it may have moved, reload too, if a page shows them.
  'learning.changed': (event) => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    getLearning(event.data.id)
      .then((response) => store.dispatch(learningUpserted(response.learning)))
      .catch((error: unknown) => {
        if (error instanceof HTTPError && error.response.status === 404) {
          store.dispatch(learningRemoved(event.data.id))
          return
        }
        console.debug('A changed learning could not be loaded', { error, learningId: event.data.id })
      })
    void mutate(learningHistoryKey(event.data.id))
    void mutate(LEARNING_FEED_KEY)
    void mutate(LEARNING_CATEGORIES_KEY)
  },
  // Pages are for the company's members, who reload what a view holds once a burst ends.
  'page.changed': (event) => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    reloadChangedPage(event.data.id)
  },
  // An ask is its asker's alone, and the event carries no content: the asker loads it by id, and
  // every other client leaves it.
  'knowledgeAsk.changed': (event) => {
    if (event.data.askedBy !== store.getState().auth.me?.user.id) {
      return
    }
    getKnowledgeAsk(event.data.id)
      .then((response) => store.dispatch(knowledgeAskUpserted(response.ask)))
      .catch((error: unknown) => console.debug('A changed ask could not be loaded', {
        error,
        askId: event.data.id,
      }))
  },
  // The list is short, so it is loaded again whole, by the company's members only.
  'learningCategories.changed': () => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    void mutate(LEARNING_CATEGORIES_KEY)
  },
  // The doc is one record, loaded again whole with its history, by the company's members only.
  'companyDoc.changed': () => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    void mutate(COMPANY_DOC_KEY)
    void mutate(COMPANY_DOC_HISTORY_KEY)
  },
  // The guidance and its versions are one answer, loaded again whole by the company's members.
  'agentGuidance.changed': () => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    void mutate(AGENT_GUIDANCE_KEY)
  },
  // The list is short and admins alone read it, so it is loaded again whole, by them.
  'modelSetting.changed': () => {
    if (!selectIsAdmin(store.getState())) {
      return
    }
    void mutate(MODEL_SETTINGS_KEY)
  },
  // A project's choice changes that project's page alone. The workspace's changes what every
  // project's page shows it follows, and what admins see each step and agent resolve to. Each
  // key refetches only while a page holds it.
  'kindModelSetting.changed': (event) => {
    const projectId = event.data.projectId
    if (projectId) {
      void mutate(getProjectModelSettingsKey(projectId))
      return
    }
    void mutate((key) => typeof key === 'string'
      && key.startsWith('v1/projects/')
      && key.endsWith('/model-settings'))
    if (selectIsAdmin(store.getState())) {
      void mutate(MODEL_SETTINGS_KEY)
    }
  },
  'connector.deleted': (event) => {
    store.dispatch(connectorDeleted(event.data.id))
    void mutate('v1/accounts')
  },
  // Moves a followed run's check along; once the run ends, it is loaded again for the
  // messages its events leave out. A run this client does not follow answers 404 and is
  // ignored.
  'connectorCheckRun.updated': (event) => {
    store.dispatch(checkRunProgressed(event.data))
    if (event.data.state !== 'finished') {
      return
    }
    if (!store.getState().connectors.checkRuns[event.data.runId]) {
      return
    }
    getCheckRun(event.data.runId)
      .then((response) => store.dispatch(checkRunLoaded(response.run)))
      .catch((error: unknown) => console.debug('A finished check run could not be loaded', {
        error,
        runId: event.data.runId,
      }))
  },
  // The event carries no content; a page showing its owner's timeline reloads it. The inbox
  // item it raised arrives as its own `actionItem.upserted`.
  // What Elysium's agents record (a Project Manager's turns, a session's turn summaries) is the
  // workspace's, which its members read.
  'timelineRecord.created': (event) => {
    const state = store.getState()
    const isOwn = event.data.createdBy === state.auth.me?.user.id
    const isWorkspaces = WORKSPACE_RECORD_OWNERS.includes(event.data.createdBy) && selectIsCompanyMember(state)
    if (!isOwn && !isWorkspaces) {
      return
    }
    void mutate((key) => typeof key === 'string' && key.startsWith('v1/timeline'))
  },
  // Its page loads it again, and the turns it has open with it, each only while a page holds it.
  // Its promises moved with it, so an open follow-up item's page loads its promise again too.
  'projectManager.changed': (event) => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    const key = getProjectManagerKey(event.data.projectId)
    void mutate((candidate) => typeof candidate === 'string'
      && (candidate.startsWith(key) || candidate.startsWith(PROMISES_KEY_PREFIX)))
  },
  // The queue's order is the server's to derive, so a page showing it loads it again, and a
  // page showing the batch loads that and its worker's log; each only while a page holds it.
  'workBatch.changed': (event) => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    void mutate(WORK_QUEUE_KEY)
    void mutate(getWorkBatchKey(event.data.id))
    void mutate(getWorkerLogKey(event.data.id))
    // Only the batch's page shows its pull requests, so their key refetches while it holds them.
    void mutate(getWorkBatchPullRequestsKey(event.data.id))
  },
  'workerPool.changed': () => {
    if (!selectIsCompanyMember(store.getState())) {
      return
    }
    void mutate(WORK_QUEUE_KEY)
  },
  // Only the project's Workers page shows its setup, so its key refetches while that page holds it.
  'projectWorkerSetup.changed': (event) => {
    void mutate(getProjectWorkerSetupKey(event.data.projectId))
  },
  // A record moved between projects, or its history grew: it is loaded by id, so a project it
  // left stops listing it, and the timeline's pages and histories that are open reload, so
  // their totals and entries follow.
  'timelineRecord.changed': (event) => {
    if (event.data.createdBy !== store.getState().auth.me?.user.id) {
      return
    }
    getTimelineRecord(event.data.id)
      .then((response) => store.dispatch(timelineRecordUpserted(response.record)))
      .catch((error: unknown) => console.debug('A changed record could not be loaded', {
        error,
        recordId: event.data.id,
      }))
    void mutate((key) => typeof key === 'string' && key.startsWith('v1/timeline'))
  },
  // A draft is its writer's alone, and the event carries no text: it is loaded by id, and one
  // that is gone was discarded.
  'draft.changed': (event) => {
    if (event.data.createdBy !== store.getState().auth.me?.user.id) {
      return
    }
    getDraft(event.data.id)
      .then((response) => store.dispatch(draftUpserted(response.draft)))
      .catch((error: unknown) => {
        if (error instanceof HTTPError && error.response.status === 404) {
          store.dispatch(draftRemoved(event.data.id))
          return
        }
        console.debug('A changed draft could not be loaded', { error, draftId: event.data.id })
      })
  },
  // Like a record, a meeting is its owner's alone, and the event carries no content: the
  // meeting is loaded by id.
  'meeting.changed': (event) => {
    if (event.data.createdBy !== store.getState().auth.me?.user.id) {
      return
    }
    getMeeting(event.data.id)
      .then((response) => store.dispatch(meetingUpserted(response.meeting)))
      .catch((error: unknown) => console.debug('A changed meeting could not be loaded', {
        error,
        meetingId: event.data.id,
      }))
  },
  'history.appended': (event) => store.dispatch(historyAppended(event.data)),
  'changeset.upserted': (event) => store.dispatch(changesetUpserted(event.data)),
  'elysia.updated': (event) => store.dispatch(elysiaUpdated(event.data)),
  'elysiaMessage.created': (event) => store.dispatch(elysiaMessageCreated(event.data)),
  'elysiaTurn.upserted': (event) => store.dispatch(elysiaTurnUpserted(event.data)),
  'environmentVariable.upserted': (event) => store.dispatch(environmentVariableUpserted(event.data)),
  'environmentVariable.deleted': (event) => store.dispatch(environmentVariableDeleted(event.data.id)),
  'githubCredential.upserted': (event) => store.dispatch(githubCredentialUpserted(event.data)),
  'githubCredential.deleted': (event) => store.dispatch(githubCredentialDeleted(event.data.id)),
  'jiraCredential.upserted': (event) => store.dispatch(jiraCredentialUpserted(event.data)),
  'jiraCredential.deleted': (event) => store.dispatch(jiraCredentialDeleted(event.data.id)),
  'llm.upserted': (event) => store.dispatch(llmUpserted(event.data)),
  'llm.deleted': (event) => store.dispatch(llmDeleted(event.data.id)),
  'llm.usage': (event) => store.dispatch(llmUsageUpdated(event.data)),
  'mailbox.upserted': (event) => store.dispatch(mailAccountUpserted(event.data)),
  'mailbox.deleted': (event) => store.dispatch(mailAccountDeleted(event.data.id)),
  'mailServer.updated': (event) => store.dispatch(mailServerUpdated(event.data)),
  'mailDomain.upserted': (event) => store.dispatch(mailDomainUpserted(event.data)),
  'mailDomain.deleted': (event) => store.dispatch(mailDomainDeleted(event.data.id)),
  'project.upserted': (event) => store.dispatch(projectUpserted(event.data)),
  'project.deleted': (event) => store.dispatch(projectDeleted(event.data.id)),
  'storageLocation.upserted': (event) => store.dispatch(storageLocationUpserted(event.data)),
  'storageLocation.deleted': (event) => store.dispatch(storageLocationDeleted(event.data.id)),
  'session.upserted': (event) => store.dispatch(codingSessionUpserted(event.data)),
  'session.deleted': (event) => store.dispatch(codingSessionDeleted(event.data.id)),
  'session.event': (event) => store.dispatch(sessionEventReceived(event.data)),
  'session.summary': (event) => store.dispatch(turnSummaryReceived(event.data)),
  'studioItem.upserted': (event) => store.dispatch(studioItemUpserted(event.data)),
  // The event carries only the id and does not say whether the delete was soft, so the
  // deleted list and the item's own page reload, each only if a view shows it: a soft delete
  // then shows the item as deleted, and a permanent one as missing.
  'studioItem.deleted': (event) => {
    store.dispatch(studioItemDeleted(event.data.id))
    void mutate('v1/studio-items?deleted=true')
    void mutate(`v1/studio-items/${event.data.id}`)
  },
  'studioAsset.created': (event) => store.dispatch(studioAssetCreated(event.data)),
  'studioFeedback.created': (event) => store.dispatch(studioFeedbackCreated(event.data)),
  'backups.status': (event) => store.dispatch(backupsStatusUpdated(event.data)),
  'updates.status': (event) => store.dispatch(updatesStatusUpdated(event.data)),
  // Also updates the signed-in person's own account, through the auth slice.
  'user.upserted': (event) => store.dispatch(userUpserted(event.data)),
  'user.deleted': (event) => store.dispatch(userDeleted(event.data.id)),
  'workspaceSettings.updated': (event) => store.dispatch(workspaceSettingsUpdated(event.data)),
  'passwordVault.updated': (event) => store.dispatch(passwordVaultUpdated(event.data)),
  'passwordVault.deleted': () => store.dispatch(passwordVaultDeleted()),
  'passwordVault.status': (event) => store.dispatch(passwordVaultStatusUpdated(event.data)),
  // Its id and state alone, since a run reads what was ingested: a member's page that shows
  // runs loads it by id, as does one that shows this run already.
  'workflowRun.changed': (event) => {
    const state = store.getState()
    // A meeting says whether its run waits for its person's approval, which only its run's
    // change tells.
    const meeting = selectAllMeetings(state).find((candidate) => candidate.runId === event.data.id)
    if (meeting) {
      getMeeting(meeting.id)
        .then((response) => store.dispatch(meetingUpserted(response.meeting)))
        .catch((error: unknown) => console.debug('A meeting whose run changed could not be loaded', {
          error,
          meetingId: meeting.id,
        }))
    }
    if (!selectIsCompanyMember(state)) {
      return
    }
    if (!state.runs.isListLoaded && !selectRunById(state, event.data.id)) {
      return
    }
    getRun(event.data.id)
      .then((response) => store.dispatch(runLoaded(response)))
      .catch((error: unknown) => console.debug('A changed run could not be loaded', {
        error,
        runId: event.data.id,
      }))
  },
  // A step's state reaches its run's page with `workflowRun.changed`; this rereads the step's
  // log, only once a page opened it.
  'stepJob.changed': (event) => {
    if (selectStepLog(store.getState(), event.data.id) === undefined) {
      return
    }
    getStepLog(event.data.runId, event.data.id)
      .then((response) => store.dispatch(stepLogLoaded({ stepId: event.data.id, log: response.log })))
      .catch((error: unknown) => console.debug('A changed step log could not be loaded', {
        error,
        stepId: event.data.id,
      }))
  },
}

function dispatchServerEvent(data: string) {
  let event: ServerEvent
  try {
    event = JSON.parse(data)
  }
  catch (error) {
    console.debug('Ignoring an event stream message that is not JSON', { error, data })
    return
  }

  // Narrowed by `type` at runtime; the lookup keeps every known type handled.
  const handler = handlers[event.type] as ((event: ServerEvent) => void) | undefined
  if (!handler) {
    console.debug('Ignoring an event of a type this build does not know', { type: event.type })
    return
  }
  handler(event)
}

// The open stream, and the reopen waiting after an error, while the stream runs.
let source: EventSource | null = null
let reopenTimer: number | undefined

// Opens the stream and keeps it open until `stopEventStream`. Browsers retry a dropped
// EventSource by themselves, but close it for good after an HTTP error response, such as a
// 502 while the API restarts; this reopens it with a backoff in that case. Starting a stream
// that already runs does nothing.
export function startEventStream() {
  if (source) {
    return
  }

  let retryDelay = EVENT_STREAM_RETRY_INITIAL_MS
  mustReloadOnHello = false
  // In the background, while the stream opens: the build this page loaded.
  readRunningBuild()

  function open() {
    const opened = new EventSource(EVENT_STREAM_PATH)
    source = opened

    opened.addEventListener('open', () => {
      retryDelay = EVENT_STREAM_RETRY_INITIAL_MS
      store.dispatch(eventStreamOpened())
    })

    opened.addEventListener('message', (message) => dispatchServerEvent(message.data))

    opened.addEventListener('error', () => {
      mustReloadOnHello = true
      store.dispatch(eventStreamLost())
      if (opened.readyState !== EventSource.CLOSED || source !== opened) {
        return
      }

      console.debug('Event stream closed by an error response; reopening', { retryDelay })
      reopenTimer = window.setTimeout(open, retryDelay)
      retryDelay = Math.min(retryDelay * 2, EVENT_STREAM_RETRY_MAX_MS)
    })
  }

  open()
}

// Closes the stream for good, such as when the person signs out.
export function stopEventStream() {
  window.clearTimeout(reopenTimer)
  source?.close()
  source = null
  store.dispatch(eventStreamStopped())
}

// Copyright © 2026 Jalapeno Labs

import type { ConnectorProvides } from './api/routes/connectorRoutes'

// Misc
import { MEETINGS_PAGE_ID } from './constants'

// Urls
export const UrlTree = {
  root: '/',
  login: '/login',
  signup: '/signup',
  recovery: '/recovery',
  pending: '/pending',
  authError: '/auth/error',
  // Where Hydra sends someone connecting an MCP client, with a challenge to answer.
  oauthLogin: '/oauth/login',
  oauthConsent: '/oauth/consent',
  actionItems: '/action-items',
  actionItemsInbox: '/action-items/inbox',
  actionItemsAll: '/action-items/all',
  actionItemsNew: '/action-items/new',
  actionItemView: '/action-items/:itemId',
  initiatives: '/action-items/initiatives',
  initiativesNew: '/action-items/initiatives/new',
  initiativeView: '/action-items/initiatives/:initiativeId',
  initiativeContainerNew: '/action-items/initiatives/:initiativeId/containers/new',
  changesets: '/action-items/changesets',
  changesetView: '/action-items/changesets/:changesetId',
  projects: '/projects',
  projectsNew: '/projects/new',
  projectView: '/projects/:projectId',
  projectModels: '/projects/:projectId/models',
  // How the project readies its workers' containers.
  projectWorkers: '/projects/:projectId/workers',
  coding: '/coding',
  codingNew: '/coding/new',
  codingSession: '/coding/:sessionId',
  studio: '/studio',
  studioNew: '/studio/new',
  studioItem: '/studio/:itemId',
  runs: '/runs',
  runView: '/runs/:runId',
  // The worker pool's queue, and each batch's own page.
  workQueue: '/work-queue',
  workBatchView: '/work-queue/:batchId',
  // Where a person edits a proposed batch, and approves it as they save.
  workBatchEdit: '/work-queue/:batchId/edit',
  // The knowledge base: it opens on its pages, and beside them are its search, asking it, its
  // learnings, and every document a citation opens.
  knowledge: '/knowledge',
  // Before `knowledgePage`, which would read `new` as a page's id.
  knowledgePageNew: '/knowledge/pages/new',
  knowledgePage: '/knowledge/pages/:pageId',
  knowledgePageEdit: '/knowledge/pages/:pageId/edit',
  knowledgeSearch: '/knowledge/search',
  knowledgeAsk: '/knowledge/ask',
  knowledgeLearnings: '/knowledge/learnings',
  knowledgeLearningNew: '/knowledge/learnings/new',
  // A learning's earlier address, which redirects to its page; its corrections and history are
  // under it.
  knowledgeLearning: '/knowledge/learnings/:learningId',
  knowledgeLearningEdit: '/knowledge/learnings/:learningId/edit',
  knowledgeDocument: '/knowledge/documents/:documentId',
  // Every page carrying a label.
  knowledgeLabel: '/knowledge/labels/:label',
  // Every meeting the reader can see: the Meetings page of the knowledge base, each one's home.
  knowledgeMeetings: '/knowledge/meetings',
  // One meeting, reviewed and corrected as a page of the knowledge base.
  knowledgeMeeting: '/knowledge/meetings/:meetingId',
  // Every result of the topbar's search, for its query.
  search: '/search',
  settings: '/settings',
  settingsAccount: '/settings/account',
  // My account's earlier addresses, which redirect to it so links and bookmarks still land.
  settingsProfile: '/settings/profile',
  settingsPersonalDetails: '/settings/personal-details',
  settingsSecurity: '/settings/security',
  // Two-factor authentication's own pages, each opened from its card on Security.
  settingsTwoFactorNew: '/settings/security/two-factor/new',
  settingsTwoFactorEdit: '/settings/security/two-factor/edit',
  settingsTwoFactorRemove: '/settings/security/two-factor/remove',
  settingsUsers: '/settings/users',
  settingsUsersAccess: '/settings/users/access',
  settingsBrowsers: '/settings/browsers',
  settingsBrowser: '/settings/browsers/:browserSessionId',
  settingsMcp: '/settings/mcp',
  // The MCP page's old address, kept as a redirect so links and bookmarks to it still land.
  settingsConnectedApps: '/settings/connected-apps',
  settingsMeetings: '/settings/meetings',
  // A meeting's earlier address, which redirects to its page in the knowledge base.
  settingsMeeting: '/settings/meetings/:meetingId',
  settingsCompanyDoc: '/settings/company-doc',
  // Learnings' earlier address, which redirects to the Knowledge page's, as do the pages under it.
  settingsLearnings: '/settings/learnings',
  settingsLearningCategories: '/settings/learning-categories',
  settingsLearningCategoryNew: '/settings/learning-categories/new',
  // Under its key and `/edit`, since a workspace may name a category `new`.
  settingsLearningCategoryEdit: '/settings/learning-categories/:categoryKey/edit',
  settingsAgentGuidance: '/settings/agent-guidance',
  settingsConnectors: '/settings/connectors',
  settingsConnectorNew: '/settings/connectors/new',
  settingsConnectorNewKind: '/settings/connectors/new/:kind',
  settingsConnector: '/settings/connectors/:connectorId',
  settingsLlms: '/settings/llms',
  // One add page per provider, named by its slug in `llmSlugByType`, as connectors are by kind.
  settingsLlmNew: '/settings/llms/new/:provider',
  settingsLlmsEdit: '/settings/llms/:llmId/edit',
  settingsLlmsAgentConfiguration: '/settings/llms/agent-configuration',
  settingsModels: '/settings/models',
  settingsWorkerPool: '/settings/worker-pool',
  settingsEnvironment: '/settings/environment',
  settingsEnvironmentNew: '/settings/environment/new',
  settingsEnvironmentEdit: '/settings/environment/:variableId/edit',
  settingsGithub: '/settings/github',
  settingsGithubNew: '/settings/github/new',
  settingsGithubEdit: '/settings/github/:credentialId/edit',
  settingsJira: '/settings/jira',
  settingsJiraNew: '/settings/jira/new',
  settingsJiraEdit: '/settings/jira/:credentialId/edit',
  settingsJiraDoneTransitions: '/settings/jira/:credentialId/done-transitions',
  settingsEmail: '/settings/email',
  settingsEmailServerNew: '/settings/email/server/new',
  settingsEmailDomainNew: '/settings/email/domains/new',
  settingsEmailMailboxNew: '/settings/email/mailboxes/new',
  settingsMailboxEdit: '/settings/email/:accountId/edit',
  settingsStorage: '/settings/storage',
  settingsStorageNew: '/settings/storage/new',
  settingsStorageAttachments: '/settings/storage/attachments',
  settingsStorageEdit: '/settings/storage/:locationId/edit',
  settingsUpdates: '/settings/updates',
  settingsBackups: '/settings/backups',
  settingsBackupsDestination: '/settings/backups/destination',
  settingsPasswords: '/settings/passwords',
} as const
export type UrlValue = typeof UrlTree[keyof typeof UrlTree]

// The query parameters that preset a new item's or initiative's project and initiative.
export const NEW_ITEM_PROJECT_PARAM = 'project'
export const NEW_ITEM_INITIATIVE_PARAM = 'initiative'

// The query parameter that narrows the add-connector grid to the kinds providing one kind of
// thing, such as meetings.
export const CONNECTOR_PROVIDES_PARAM = 'provides'

// The query parameter that opens a Jira site's done transitions on one project, such as from
// a close still waiting on the choice.
export const DONE_TRANSITION_PROJECT_PARAM = 'project'

// The query parameter that starts a new coding session from an action item.
export const NEW_SESSION_ITEM_PARAM = 'item'

// The query parameter naming the turn a session's conversation opens at, as a turn summary on a
// project's timeline links to it.
export const FOCUS_TURN_PARAM = 'turn'

// The query parameter that holds what the search page searches for. Its kind of result is its
// open tab (`TAB_PARAM`).
export const SEARCH_QUERY_PARAM = 'q'

// The query parameter naming the passage a knowledge document or page opens at, which a citation
// names.
export const KNOWLEDGE_PASSAGE_PARAM = 'passage'
// The knowledge search's tab that searches pages alone.
export const KNOWLEDGE_PAGES_SCOPE = 'pages'
// The page a new page goes under, chosen already when it is added from the tree.
export const NEW_PAGE_PARENT_PARAM = 'parent'
// The project the Meetings page lists the meetings of, as a project's Meetings group in the tree
// opens it.
export const MEETINGS_PROJECT_PARAM = 'project'

// The query parameters the sign-in page reads: where to go afterwards, whether to ask for
// the second factor or to confirm a recent sign-in, and a notice to show.
export const LOGIN_RETURN_TO_PARAM = 'return_to'
export const LOGIN_AAL_PARAM = 'aal'
export const LOGIN_REFRESH_PARAM = 'refresh'
export const LOGIN_NOTICE_PARAM = 'notice'

// Kratos names the flow a page continues in this parameter when it sends the browser to one.
export const KRATOS_FLOW_PARAM = 'flow'

// The query parameter naming a page's open tab (`useTabParam`), so a tab survives a reload and
// a link can open one.
export const TAB_PARAM = 'tab'

// What a knowledge page's History tab shows: one version read through, or two compared. They live
// in the address, so Back returns to the list and a comparison can be linked to.
export const PAGE_HISTORY_TAB = 'history'
// The project's tab listing the pages linked to it.
export const PROJECT_DOCS_TAB = 'docs'
export const PAGE_VERSION_PARAM = 'version'
export const PAGE_COMPARE_FROM_PARAM = 'from'
export const PAGE_COMPARE_TO_PARAM = 'to'

// Settings
// The first page in the sidebar; the app has no home page.
export const UNKNOWN_ROUTE_REDIRECT_TO: UrlValue = UrlTree.actionItems

// Link factories

export function getActionItemViewUrl(itemId: string) {
  return UrlTree.actionItemView.replace(':itemId', itemId)
}

export function getInitiativeViewUrl(initiativeId: string) {
  return UrlTree.initiativeView.replace(':initiativeId', initiativeId)
}

export function getNewInitiativeContainerUrl(initiativeId: string) {
  return UrlTree.initiativeContainerNew.replace(':initiativeId', initiativeId)
}

export function getChangesetViewUrl(changesetId: string) {
  return UrlTree.changesetView.replace(':changesetId', changesetId)
}

// New items can start in a project or an initiative, from that project's or initiative's page.
export function getNewActionItemUrl(preset: { projectId?: string, initiativeId?: string }) {
  const params = new URLSearchParams()
  if (preset.projectId) {
    params.set(NEW_ITEM_PROJECT_PARAM, preset.projectId)
  }
  if (preset.initiativeId) {
    params.set(NEW_ITEM_INITIATIVE_PARAM, preset.initiativeId)
  }
  const query = params.toString()
  if (!query) {
    return UrlTree.actionItemsNew
  }
  return `${UrlTree.actionItemsNew}?${query}`
}

// New initiatives can start in a project, from its page.
export function getNewInitiativeUrl(projectId: string) {
  return `${UrlTree.initiativesNew}?${NEW_ITEM_PROJECT_PARAM}=${encodeURIComponent(projectId)}`
}

export function getProjectViewUrl(projectId: string) {
  return UrlTree.projectView.replace(':projectId', projectId)
}

export function getProjectDocsUrl(projectId: string) {
  return `${getProjectViewUrl(projectId)}?${TAB_PARAM}=${PROJECT_DOCS_TAB}`
}

export function getProjectModelsUrl(projectId: string) {
  return UrlTree.projectModels.replace(':projectId', projectId)
}

export function getProjectWorkersUrl(projectId: string) {
  return UrlTree.projectWorkers.replace(':projectId', projectId)
}

export function getRunViewUrl(runId: string) {
  return UrlTree.runView.replace(':runId', runId)
}

export function getWorkBatchViewUrl(batchId: string) {
  return UrlTree.workBatchView.replace(':batchId', batchId)
}

export function getWorkBatchEditUrl(batchId: string) {
  return UrlTree.workBatchEdit.replace(':batchId', batchId)
}

export function getMeetingUrl(meetingId: string) {
  return UrlTree.knowledgeMeeting.replace(':meetingId', meetingId)
}

// The Meetings page, narrowed to the meetings with a topic on one project.
export function getProjectMeetingsUrl(projectId: string) {
  const params = new URLSearchParams({ [MEETINGS_PROJECT_PARAM]: projectId })
  return `${UrlTree.knowledgeMeetings}?${params.toString()}`
}

export function getConnectorUrl(connectorId: string) {
  return UrlTree.settingsConnector.replace(':connectorId', connectorId)
}

export function getNewConnectorUrl(kind: string) {
  return UrlTree.settingsConnectorNewKind.replace(':kind', kind)
}

export function getConnectorPickerUrl(provides: ConnectorProvides) {
  return `${UrlTree.settingsConnectorNew}?${CONNECTOR_PROVIDES_PARAM}=${encodeURIComponent(provides)}`
}

// A page opened on one of its tabs (`useTabParam`), such as Email's Domains after adding a
// domain.
export function getTabUrl(path: UrlValue, tab: string) {
  const params = new URLSearchParams({ [TAB_PARAM]: tab })
  return `${path}?${params.toString()}`
}

export function getNewLlmUrl(providerSlug: string) {
  return UrlTree.settingsLlmNew.replace(':provider', providerSlug)
}

export function getLlmEditUrl(llmId: string) {
  return UrlTree.settingsLlmsEdit.replace(':llmId', llmId)
}

export function getEnvironmentVariableEditUrl(variableId: string) {
  return UrlTree.settingsEnvironmentEdit.replace(':variableId', variableId)
}

export function getLearningEditUrl(learningId: string) {
  return UrlTree.knowledgeLearningEdit.replace(':learningId', learningId)
}

// The seeded pages the app shows as a page of its own rather than as their text: the tree opens each
// there, and its address as a page lands there.
export const pageAddressById: Readonly<Partial<Record<string, string>>> = {
  [MEETINGS_PAGE_ID]: UrlTree.knowledgeMeetings,
}

// A page of the knowledge base, opened at one of its passages when a citation names one.
export function getKnowledgePageUrl(pageId: string, passage?: number) {
  const url = UrlTree.knowledgePage.replace(':pageId', pageId)
  if (passage === undefined) {
    return url
  }
  return `${url}?${KNOWLEDGE_PASSAGE_PARAM}=${passage}`
}

export function getPageHistoryUrl(pageId: string) {
  return `${getKnowledgePageUrl(pageId)}?${TAB_PARAM}=${PAGE_HISTORY_TAB}`
}

export function getPageVersionUrl(pageId: string, version: number) {
  return `${getPageHistoryUrl(pageId)}&${PAGE_VERSION_PARAM}=${version}`
}

export function getPageCompareUrl(pageId: string, from: number, to: number) {
  return `${getPageHistoryUrl(pageId)}&${PAGE_COMPARE_FROM_PARAM}=${from}&${PAGE_COMPARE_TO_PARAM}=${to}`
}

// The new page form, with the page it goes under already chosen.
export function getNewChildPageUrl(parentId: string) {
  const params = new URLSearchParams({ [NEW_PAGE_PARENT_PARAM]: parentId })
  return `${UrlTree.knowledgePageNew}?${params.toString()}`
}

export function getKnowledgePageEditUrl(pageId: string) {
  return UrlTree.knowledgePageEdit.replace(':pageId', pageId)
}

export function getKnowledgeLabelUrl(label: string) {
  return UrlTree.knowledgeLabel.replace(':label', encodeURIComponent(label))
}

// A document of the knowledge base, opened at one of its passages when a citation names one.
export function getKnowledgeDocumentUrl(documentId: string, passage?: number) {
  const url = UrlTree.knowledgeDocument.replace(':documentId', documentId)
  if (passage === undefined) {
    return url
  }
  return `${url}?${KNOWLEDGE_PASSAGE_PARAM}=${passage}`
}

// Where a citation opens: its page, read as it reads, when it was indexed from one, or else the
// document, which is all a timeline record has for anyone but its owner.
export function getKnowledgeCitationUrl(citation: { documentId: string, pageId: string | null }, passage?: number) {
  if (citation.pageId) {
    return getKnowledgePageUrl(citation.pageId, passage)
  }
  return getKnowledgeDocumentUrl(citation.documentId, passage)
}

// The knowledge search for `query`, as the topbar's results open it.
export function getKnowledgeSearchUrl(query: string) {
  const params = new URLSearchParams({ [SEARCH_QUERY_PARAM]: query })
  return `${UrlTree.knowledgeSearch}?${params.toString()}`
}

// The knowledge search of pages alone, for `query` when there is one.
export function getKnowledgePagesSearchUrl(query?: string) {
  const params = new URLSearchParams({ [TAB_PARAM]: KNOWLEDGE_PAGES_SCOPE })
  if (query) {
    params.set(SEARCH_QUERY_PARAM, query)
  }
  return `${UrlTree.knowledgeSearch}?${params.toString()}`
}

export function getLearningCategoryEditUrl(categoryKey: string) {
  return UrlTree.settingsLearningCategoryEdit.replace(':categoryKey', categoryKey)
}

export function getGithubCredentialEditUrl(credentialId: string) {
  return UrlTree.settingsGithubEdit.replace(':credentialId', credentialId)
}

export function getJiraCredentialEditUrl(credentialId: string) {
  return UrlTree.settingsJiraEdit.replace(':credentialId', credentialId)
}

export function getJiraDoneTransitionsUrl(credentialId: string, projectKey?: string) {
  const url = UrlTree.settingsJiraDoneTransitions.replace(':credentialId', credentialId)
  if (!projectKey) {
    return url
  }
  return `${url}?${DONE_TRANSITION_PROJECT_PARAM}=${encodeURIComponent(projectKey)}`
}

export function getMailboxEditUrl(accountId: string) {
  return UrlTree.settingsMailboxEdit.replace(':accountId', accountId)
}

export function getStorageLocationEditUrl(locationId: string) {
  return UrlTree.settingsStorageEdit.replace(':locationId', locationId)
}

// The Coding page opens this session's conversation, then returns to its own address.
export function getCodingSessionUrl(sessionId: number) {
  return UrlTree.codingSession.replace(':sessionId', String(sessionId))
}

// New session, started from this item.
export function getNewCodingSessionUrl(actionItemId: string) {
  return `${UrlTree.codingNew}?${NEW_SESSION_ITEM_PARAM}=${encodeURIComponent(actionItemId)}`
}

// Every search result for the query, as the topbar's View all opens it.
export function getSearchUrl(query: string) {
  const params = new URLSearchParams({ [SEARCH_QUERY_PARAM]: query })
  return `${UrlTree.search}?${params.toString()}`
}

// One of Security's tabs; General, its first, needs none.
export function getSecurityTabUrl(tab: 'passkeys' | 'lookup' | 'sessions') {
  return `${UrlTree.settingsSecurity}?${TAB_PARAM}=${tab}`
}

export function getStudioItemUrl(itemId: string) {
  return UrlTree.studioItem.replace(':itemId', itemId)
}

// A session's conversation, opened at one of its turns: on the Coding page, or on its Studio
// item's page for a Studio session.
export function getSessionTurnUrl(sessionId: number, turnId: string, studioItemId: string | null) {
  const conversation = studioItemId
    ? getStudioItemUrl(studioItemId)
    : getCodingSessionUrl(sessionId)
  const params = new URLSearchParams({ [FOCUS_TURN_PARAM]: turnId })
  return `${conversation}?${params.toString()}`
}

type LoginOptions = {
  // A path on this origin to open after signing in.
  returnTo?: string
  // Ask for the authenticator code or a lookup code, after the password or passkey.
  secondFactor?: boolean
  // Confirm a recent sign-in, which changing a password, passkey, or authenticator needs.
  refresh?: boolean
  notice?: 'disabled'
}

export function getLoginUrl(options: LoginOptions = {}) {
  const params = new URLSearchParams()
  if (options.returnTo) {
    params.set(LOGIN_RETURN_TO_PARAM, options.returnTo)
  }
  if (options.secondFactor) {
    params.set(LOGIN_AAL_PARAM, 'aal2')
  }
  if (options.refresh) {
    params.set(LOGIN_REFRESH_PARAM, 'true')
  }
  if (options.notice) {
    params.set(LOGIN_NOTICE_PARAM, options.notice)
  }
  const query = params.toString()
  if (!query) {
    return UrlTree.login
  }
  return `${UrlTree.login}?${query}`
}

// Copyright © 2026 Jalapeno Labs

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
  changesets: '/action-items/changesets',
  changesetView: '/action-items/changesets/:changesetId',
  projects: '/projects',
  projectsNew: '/projects/new',
  projectView: '/projects/:projectId',
  coding: '/coding',
  codingSession: '/coding/:sessionId',
  studio: '/studio',
  studioItem: '/studio/:itemId',
  settings: '/settings',
  settingsPersonalDetails: '/settings/personal-details',
  settingsSecurity: '/settings/security',
  settingsUsers: '/settings/users',
  settingsConnectedApps: '/settings/connected-apps',
  settingsLlms: '/settings/llms',
  settingsLlmsAddCodexOauth: '/settings/llms/add-codex-oauth',
  settingsLlmsAddClaudeCodeOauth: '/settings/llms/add-claude-code-oauth',
  settingsLlmsAddClaudeApiKey: '/settings/llms/add-claude-api-key',
  settingsLlmsAddOpenaiApiKey: '/settings/llms/add-openai-api-key',
  settingsLlmsEdit: '/settings/llms/:llmId/edit',
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
  settingsSatellites: '/settings/satellites',
  settingsEmail: '/settings/email',
  settingsStorage: '/settings/storage',
  settingsStorageNew: '/settings/storage/new',
  settingsStorageEdit: '/settings/storage/:locationId/edit',
} as const
export type UrlValue = typeof UrlTree[keyof typeof UrlTree]

// The query parameters that preset a new item's or initiative's project and initiative.
export const NEW_ITEM_PROJECT_PARAM = 'project'
export const NEW_ITEM_INITIATIVE_PARAM = 'initiative'

// The query parameter that opens a Jira site's done transitions on one project, such as from
// a close still waiting on the choice.
export const DONE_TRANSITION_PROJECT_PARAM = 'project'

// The query parameter that opens the Coding page's New session dialog started from an item.
export const NEW_SESSION_ITEM_PARAM = 'item'

// The query parameters the sign-in page reads: where to go afterwards, whether to ask for
// the second factor or to confirm a recent sign-in, and a notice to show.
export const LOGIN_RETURN_TO_PARAM = 'return_to'
export const LOGIN_AAL_PARAM = 'aal'
export const LOGIN_REFRESH_PARAM = 'refresh'
export const LOGIN_NOTICE_PARAM = 'notice'

// Kratos names the flow a page continues in this parameter when it sends the browser to one.
export const KRATOS_FLOW_PARAM = 'flow'

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

export function getEnvironmentVariableEditUrl(variableId: string) {
  return UrlTree.settingsEnvironmentEdit.replace(':variableId', variableId)
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

export function getStorageLocationEditUrl(locationId: string) {
  return UrlTree.settingsStorageEdit.replace(':locationId', locationId)
}

// The Coding page opens this session's conversation, then returns to its own address.
export function getCodingSessionUrl(sessionId: number) {
  return UrlTree.codingSession.replace(':sessionId', String(sessionId))
}

// The Coding page opens New session started from this item, then returns to its own address.
export function getNewCodingSessionUrl(actionItemId: string) {
  return `${UrlTree.coding}?${NEW_SESSION_ITEM_PARAM}=${encodeURIComponent(actionItemId)}`
}

export function getStudioItemUrl(itemId: string) {
  return UrlTree.studioItem.replace(':itemId', itemId)
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

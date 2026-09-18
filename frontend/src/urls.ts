// Copyright © 2026 Jalapeno Labs

// Urls
export const UrlTree = {
  root: '/',
  actionItems: '/action-items',
  actionItemsInbox: '/action-items/inbox',
  actionItemsAll: '/action-items/all',
  actionItemsNew: '/action-items/new',
  actionItemView: '/action-items/:itemId',
  initiatives: '/action-items/initiatives',
  initiativesNew: '/action-items/initiatives/new',
  initiativeView: '/action-items/initiatives/:initiativeId',
  studio: '/studio',
  projects: '/projects',
  projectsNew: '/projects/new',
  projectView: '/projects/:projectId',
  coding: '/coding',
  codingSession: '/coding/:sessionId',
  settings: '/settings',
  settingsPersonalDetails: '/settings/personal-details',
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

// The query parameter that opens the Coding page's New session dialog started from an item.
export const NEW_SESSION_ITEM_PARAM = 'item'

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

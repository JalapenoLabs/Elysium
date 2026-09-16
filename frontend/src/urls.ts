// Copyright © 2026 Jalapeno Labs

// Urls
export const UrlTree = {
  root: '/',
  actionItems: '/action-items',
  studio: '/studio',
  projects: '/projects',
  projectsNew: '/projects/new',
  projectView: '/projects/:projectId',
  coding: '/coding',
  settings: '/settings',
  settingsPersonalDetails: '/settings/personal-details',
  settingsLlms: '/settings/llms',
  settingsLlmsAddCodexOauth: '/settings/llms/add-codex-oauth',
  settingsLlmsAddClaudeCodeOauth: '/settings/llms/add-claude-code-oauth',
  settingsLlmsAddClaudeApiKey: '/settings/llms/add-claude-api-key',
  settingsLlmsAddOpenaiApiKey: '/settings/llms/add-openai-api-key',
  settingsLlmsEdit: '/settings/llms/:llmId/edit',
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

// Settings
// The first page in the sidebar; the app has no home page.
export const UNKNOWN_ROUTE_REDIRECT_TO: UrlValue = UrlTree.actionItems

// Link factories

export function getProjectViewUrl(projectId: string) {
  return UrlTree.projectView.replace(':projectId', projectId)
}

export function getGithubCredentialEditUrl(credentialId: string) {
  return UrlTree.settingsGithubEdit.replace(':credentialId', credentialId)
}

export function getStorageLocationEditUrl(locationId: string) {
  return UrlTree.settingsStorageEdit.replace(':locationId', locationId)
}

// The Coding page opens this session's conversation, then drops the parameter.
export function getCodingSessionUrl(sessionId: string) {
  return `${UrlTree.coding}?session=${encodeURIComponent(sessionId)}`
}

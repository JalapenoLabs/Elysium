// Copyright © 2026 Jalapeno Labs

// Urls
export const UrlTree = {
  root: '/',
  actionItems: '/action-items',
  studio: '/studio',
  projects: '/projects',
  projectsNew: '/projects/new',
  coding: '/coding',
  settings: '/settings',
  settingsPersonalDetails: '/settings/personal-details',
  settingsLlms: '/settings/llms',
  settingsLlmsAddCodexOauth: '/settings/llms/add-codex-oauth',
  settingsLlmsAddClaudeCodeOauth: '/settings/llms/add-claude-code-oauth',
  settingsLlmsAddClaudeApiKey: '/settings/llms/add-claude-api-key',
  settingsLlmsAddOpenaiApiKey: '/settings/llms/add-openai-api-key',
  settingsLlmsEdit: '/settings/llms/:llmId/edit',
  settingsSatellites: '/settings/satellites',
  settingsEmail: '/settings/email',
} as const
export type UrlValue = typeof UrlTree[keyof typeof UrlTree]

// Settings
export const UNKNOWN_ROUTE_REDIRECT_TO: UrlValue = UrlTree.root

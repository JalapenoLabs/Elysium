// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { LlmType } from '../../../api/routes/llmRoutes'

// Misc
import { UrlTree } from '../../../urls'

// One setup step. The text is translated; commands, file paths, and URLs are not.
export type LlmSetupStep = {
  textKey: ParseKeys<'llms'>
  command?: string
  link?: string
}

// Each provider type has its own add page, reached from the Add LLM menu.
export const addLlmUrlByType = {
  'chatgpt-oauth': UrlTree.settingsLlmsAddCodexOauth,
  'claude-code-oauth': UrlTree.settingsLlmsAddClaudeCodeOauth,
  'claude-api-token': UrlTree.settingsLlmsAddClaudeApiKey,
  'chatgpt-api-token': UrlTree.settingsLlmsAddOpenaiApiKey,
} as const satisfies Record<LlmType, string>

// How to obtain each type's secret token, shown beside the form as a checklist.
export const llmSetupStepsByType = {
  'chatgpt-oauth': [
    { textKey: 'setup.chatgpt-oauth.install', command: 'npm install -g @openai/codex' },
    { textKey: 'setup.chatgpt-oauth.login', command: 'codex login' },
    { textKey: 'setup.chatgpt-oauth.open', command: '~/.codex/auth.json' },
    { textKey: 'setup.chatgpt-oauth.copy' },
  ],
  'claude-code-oauth': [
    { textKey: 'setup.claude-code-oauth.install', command: 'npm install -g @anthropic-ai/claude-code' },
    { textKey: 'setup.claude-code-oauth.run', command: 'claude setup-token' },
    { textKey: 'setup.claude-code-oauth.signIn' },
    { textKey: 'setup.claude-code-oauth.copy' },
    { textKey: 'setup.claude-code-oauth.expiry' },
  ],
  'claude-api-token': [
    { textKey: 'setup.claude-api-token.open', link: 'https://console.anthropic.com/settings/keys' },
    { textKey: 'setup.claude-api-token.create' },
    { textKey: 'setup.claude-api-token.copy' },
  ],
  'chatgpt-api-token': [
    { textKey: 'setup.chatgpt-api-token.open', link: 'https://platform.openai.com/api-keys' },
    { textKey: 'setup.chatgpt-api-token.create' },
    { textKey: 'setup.chatgpt-api-token.copy' },
    { textKey: 'setup.chatgpt-api-token.billing', link: 'https://platform.openai.com/settings/organization/billing' },
  ],
} as const satisfies Record<LlmType, readonly LlmSetupStep[]>

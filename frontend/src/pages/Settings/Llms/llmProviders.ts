// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { LlmType } from '../../../api/routes/llmRoutes'

// One setup step. The text is translated; commands, file paths, and URLs are not.
export type LlmSetupStep = {
  textKey: ParseKeys<'llms'>
  command?: string
  link?: string
}

// Each provider type has its own add page, `/settings/llms/new/<slug>` (`getNewLlmUrl`), reached
// from the Add LLM menu. The slug names the provider the way people know it, such as Codex, rather
// than by the API's type. `/settings/llms/add-<slug>` redirects to it, so links to that still land.
export const llmSlugByType = {
  'chatgpt-oauth': 'codex-oauth',
  'claude-code-oauth': 'claude-code-oauth',
  'claude-api-token': 'claude-api-key',
  'chatgpt-api-token': 'openai-api-key',
  'claude-account': 'claude-account',
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
  'claude-account': [
    { textKey: 'setup.claude-account.start' },
    { textKey: 'setup.claude-account.open' },
    { textKey: 'setup.claude-account.code' },
    { textKey: 'setup.claude-account.kept' },
  ],
} as const satisfies Record<LlmType, readonly LlmSetupStep[]>

// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  ConfigurationHold,
  ConfigurationSkippedKind,
  ConfigurationSkipReason,
} from '../api/routes/agentConfigurationRoutes'

// How the configuration repository's state reads wherever it shows: the settings page, the work
// queue, a batch, and a Project Manager. Pure, so it is tested without rendering.

export const holdLabelKeys = {
  unset: 'agentConfiguration.hold.unset',
  fetching: 'agentConfiguration.hold.fetching',
  failed: 'agentConfiguration.hold.failed',
} as const satisfies Record<ConfigurationHold['reason'], ParseKeys<'llms'>>

// A hold that lifts on its own in moments is news, not a warning.
export const holdAlertStatuses = {
  unset: 'warning',
  fetching: 'accent',
  failed: 'warning',
} as const satisfies Record<ConfigurationHold['reason'], 'warning' | 'accent'>

export const skippedKindLabelKeys = {
  hook: 'agentConfiguration.skippedKinds.hook',
  plugin: 'agentConfiguration.skippedKinds.plugin',
  mcpServer: 'agentConfiguration.skippedKinds.mcpServer',
  environment: 'agentConfiguration.skippedKinds.environment',
  setting: 'agentConfiguration.skippedKinds.setting',
} as const satisfies Record<ConfigurationSkippedKind, ParseKeys<'llms'>>

export const skipReasonLabelKeys = {
  missingCommand: 'agentConfiguration.skipReasons.missingCommand',
  missingPath: 'agentConfiguration.skipReasons.missingPath',
  loopback: 'agentConfiguration.skipReasons.loopback',
  notInstalled: 'agentConfiguration.skipReasons.notInstalled',
  reserved: 'agentConfiguration.skipReasons.reserved',
} as const satisfies Record<ConfigurationSkipReason, ParseKeys<'llms'>>

// How many characters of a commit id are shown, as GitHub abbreviates one.
const SHORT_COMMIT_LENGTH = 7

export function shortCommit(commit: string) {
  return commit.slice(0, SHORT_COMMIT_LENGTH)
}

// Where a commit of a github.com repository is shown, or null for another host, whose pages
// Elysium does not know.
export function commitUrl(repository: string, commit: string) {
  let url: URL
  try {
    url = new URL(repository)
  }
  catch (error) {
    console.debug('commitUrl could not read the repository as a URL', { repository, error })
    return null
  }
  if (url.hostname !== 'github.com') {
    return null
  }
  const path = url.pathname
    .replace(/\/+$/, '')
    .replace(/\.git$/, '')
  return `https://github.com${path}/commit/${commit}`
}

// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `SkippedKind` in api/src/runner/configuration/adapt.rs.
export const CONFIGURATION_SKIPPED_KINDS = [ 'hook', 'plugin', 'mcpServer', 'environment', 'setting' ] as const
export type ConfigurationSkippedKind = typeof CONFIGURATION_SKIPPED_KINDS[number]

// Mirrors `SkipReason` in api/src/runner/configuration/adapt.rs.
export const CONFIGURATION_SKIP_REASONS = [
  'missingCommand',
  'missingPath',
  'loopback',
  'notInstalled',
  'reserved',
] as const
export type ConfigurationSkipReason = typeof CONFIGURATION_SKIP_REASONS[number]

// Something the configuration repository held that an agent's container cannot run, and so
// left out of every checkout.
export type ConfigurationSkipped = {
  kind: ConfigurationSkippedKind
  // As the repository names it: a hook's event and command, a plugin, an MCP server, an
  // environment key, or a setting.
  name: string
  reason: ConfigurationSkipReason
  // The command, path, or address that decided it; never an environment value.
  detail?: string
}

// A checkout of the configuration repository.
export type ConfigurationCheckout = {
  // The full 40-character commit id.
  commit: string
  subject: string
  fetchedAt: string
  skipped: ConfigurationSkipped[]
}

// The configuration a batch's worker runs on. Mirrors `Checkout` in
// api/src/runner/configuration/mod.rs.
export type BatchConfiguration = ConfigurationCheckout & {
  repository: string
}

// Why workers and Project Managers wait. Mirrors `Hold` in api/src/agent_configuration/mod.rs.
export type ConfigurationHold =
  | { reason: 'unset' }
  | { reason: 'fetching' }
  | { reason: 'failed', error: string, failedAt: string }

// The configuration repository's last fetch, as the settings page shows it.
export type AgentConfiguration = {
  // Null while workers and Project Managers may start.
  hold: ConfigurationHold | null
  // The last good checkout of the repository and token the settings name.
  checkout: ConfigurationCheckout | null
}

// Asks the engine to fetch the configuration repository now. It answers at once; how the fetch
// went arrives as `workspaceSettings.updated`.
export function fetchAgentConfiguration() {
  return apiClient.post('v1/workspace-settings/agent-configuration/fetch')
}

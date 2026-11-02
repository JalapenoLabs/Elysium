// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type {
  CheckOutcome,
  CheckSpec,
  CheckState,
  Connector,
  ConnectorKind,
  ConnectorStatus,
} from '../../../api/routes/connectorRoutes'

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const statusColors = {
  active: 'success',
  syncing: 'accent',
  error: 'danger',
  paused: 'default',
} as const satisfies Record<ConnectorStatus, ChipColor>

export const checkStateColors = {
  queued: 'default',
  running: 'accent',
  passed: 'success',
  failed: 'danger',
  skipped: 'warning',
} as const satisfies Record<CheckState, ChipColor>

// What a connector reads through, as its row names it.
export function accountNameOf(connector: Connector) {
  return connector.account?.name ?? ''
}

// The hub's summary row, from one pass over the connectors.
export function summarize(connectors: Connector[]) {
  let active = 0
  let errors = 0
  let records = 0
  for (const connector of connectors) {
    records += connector.records
    if (connector.status === 'error') {
      errors += 1
    }
    if (connector.status !== 'paused') {
      active += 1
    }
  }
  return { total: connectors.length, active, errors, records } as const
}

// The connectors a search and a status filter keep. The search matches the kind's name and
// the account's, so "jira" and "work" both find a Jira connector on a "Work" account.
export function filterConnectors(
  connectors: Connector[],
  search: string,
  status: ConnectorStatus | 'all',
  kindName: (kind: ConnectorKind) => string,
) {
  const needle = search.trim().toLowerCase()
  return connectors.filter((connector) => {
    if (status !== 'all' && connector.status !== status) {
      return false
    }
    if (!needle) {
      return true
    }
    const haystack = `${kindName(connector.kind)} ${accountNameOf(connector)}`.toLowerCase()
    return haystack.includes(needle)
  })
}

// The words beside a check's state: why it waited, Elysium's own reason, the provider's
// message, or what it counted, in that order.
export function describeOutcome(
  t: TFunction<'connectors'>,
  outcome: CheckOutcome,
  labelOf: (checkId: string) => string,
  checkId: string,
) {
  if (outcome.blockedBy) {
    return t('checks.waitsFor', { check: labelOf(outcome.blockedBy) })
  }
  if (outcome.reason) {
    return t(`checks.reasons.${outcome.reason}`, { defaultValue: outcome.reason })
  }
  if (outcome.message && outcome.state === 'failed') {
    return outcome.message
  }
  if (outcome.count !== null) {
    return t(`checks.counts.${checkId}`, { count: outcome.count, defaultValue: String(outcome.count) })
  }
  return outcome.message ?? ''
}

// Whether a finished run leaves a required check that did not pass.
export function hasRequiredFailure(
  specs: CheckSpec[],
  outcomes: Array<CheckOutcome & { checkId: string }>,
) {
  const required = new Set(specs.filter((spec) => spec.required).map((spec) => spec.id))
  return outcomes.some((outcome) => required.has(outcome.checkId) && outcome.state !== 'passed')
}

// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { BrowserState } from '../../../api/routes/browserSessionRoutes'
import type {
  CheckOutcome,
  CheckSpec,
  CheckState,
  Connector,
  ConnectorKind,
  ConnectorKindDefinition,
  ConnectorStatus,
  SignInState,
} from '../../../api/routes/connectorRoutes'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const statusColors = {
  active: 'success',
  syncing: 'accent',
  error: 'danger',
  paused: 'default',
} as const satisfies Record<ConnectorStatus, ChipColor>

export const signInLabelKeys = {
  signed_in: 'signIn.states.signedIn',
  signed_out: 'signIn.states.signedOut',
  unknown: 'signIn.states.unknown',
} as const satisfies Record<SignInState, ParseKeys<'connectors'>>

export const signInChipColors = {
  signed_in: 'success',
  signed_out: 'danger',
  unknown: 'default',
} as const satisfies Record<SignInState, ChipColor>

export const browserStateLabelKeys = {
  starting: 'states.starting',
  running: 'states.running',
  failed: 'states.failed',
} as const satisfies Record<BrowserState, ParseKeys<'browsers'>>

export const browserStateChipColors = {
  starting: 'warning',
  running: 'success',
  failed: 'danger',
} as const satisfies Record<BrowserState, ChipColor>

export const checkStateColors = {
  queued: 'default',
  running: 'accent',
  passed: 'success',
  failed: 'danger',
  skipped: 'warning',
} as const satisfies Record<CheckState, ChipColor>

// The name of the account a connector owns, as its row shows it. A browser goes unnamed: it is
// named for its kind, which the row already shows, and its person meets it only as the
// connector.
export function accountNameOf(connector: Connector) {
  if (connector.account?.kind === 'browser') {
    return ''
  }
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

// The kinds the add-connector grid shows for a search, in the hub's order. It matches each
// kind's name and what it reads, so "mail" finds Outlook and Gmail.
export function searchKinds(
  kinds: ConnectorKind[],
  search: string,
  wordsOf: (kind: ConnectorKind) => string,
) {
  const needle = search.trim().toLowerCase()
  if (!needle) {
    return kinds
  }
  return kinds.filter((kind) => wordsOf(kind).toLowerCase().includes(needle))
}

// What a new connector of a kind read through a browser starts reading: everything each of its
// selections can pick, since nothing can be listed until its person signs in. It is narrowed on
// the connector's page afterwards.
export function startingContent(definition: Pick<ConnectorKindDefinition, 'content'>) {
  const content: Record<string, unknown> = {}
  for (const field of definition.content) {
    if (field.field.type === 'selection') {
      content[field.id] = '*'
    }
  }
  return content
}

// Whether a finished run leaves a required check that did not pass.
export function hasRequiredFailure(
  specs: CheckSpec[],
  outcomes: Array<CheckOutcome & { checkId: string }>,
) {
  const required = new Set(specs.filter((spec) => spec.required).map((spec) => spec.id))
  return outcomes.some((outcome) => required.has(outcome.checkId) && outcome.state !== 'passed')
}

// The refusals a connector's browser can meet, worded here rather than in the API's English.
const errorKeyByCode = {
  browser_ports_exhausted: 'errors.browserPortsExhausted',
  browser_not_started: 'errors.browserNotStarted',
} as const satisfies Record<string, ParseKeys<'connectors'>>

function isWordedCode(code: string | null): code is keyof typeof errorKeyByCode {
  return code !== null && Object.hasOwn(errorKeyByCode, code)
}

// What a failed connector request says: Elysium's own words for a coded refusal, otherwise the
// API's message, which names what went wrong and never holds a secret.
export function connectorErrorMessage(error: unknown, t: TFunction<[ 'connectors', 'common' ]>) {
  const code = getApiErrorCode(error)
  if (isWordedCode(code)) {
    return t(errorKeyByCode[code])
  }
  return getApiErrorMessage(error) ?? t('common:errors.unexpected')
}

// The connector that owns the browser `browserSessionId`, if one does: each browser is one
// connector's account.
export function connectorOwningBrowser(connectors: Connector[], browserSessionId: string) {
  return connectors.find((connector) => connector.account?.kind === 'browser'
    && connector.account.id === browserSessionId) ?? null
}

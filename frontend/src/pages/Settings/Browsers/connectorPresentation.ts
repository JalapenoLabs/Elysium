// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { ConnectorKind, SignInState } from '../../../api/routes/connectorRoutes'

// Misc
import { CONNECTOR_KINDS } from '../../../api/routes/connectorRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

// Where each connector's service lives, so a browser on one of these hosts can read its mail.
// Mirrors `mailbox_origins` in api/src/connectors/webmail.rs.
const hostsByConnectorKind = {
  outlook: [ 'outlook.office.com', 'outlook.office365.com', 'outlook.live.com' ],
  gmail: [ 'mail.google.com' ],
} as const satisfies Record<ConnectorKind, readonly string[]>

// The connector that reads a browser started at `startUrl`, if one does.
export function connectorKindFor(startUrl: string): ConnectorKind | null {
  if (!URL.canParse(startUrl)) {
    return null
  }
  const host = new URL(startUrl).hostname
  const kind = CONNECTOR_KINDS.find((candidate) => {
    const hosts: readonly string[] = hostsByConnectorKind[candidate]
    return hosts.includes(host)
  })
  return kind ?? null
}

export const connectorLabelKeys = {
  outlook: 'connectors.kinds.outlook',
  gmail: 'connectors.kinds.gmail',
} as const satisfies Record<ConnectorKind, ParseKeys<'browsers'>>

export const signInLabelKeys = {
  signed_in: 'connectors.signIn.signedIn',
  signed_out: 'connectors.signIn.signedOut',
  unknown: 'connectors.signIn.unknown',
} as const satisfies Record<SignInState, ParseKeys<'browsers'>>

export const signInChipColors = {
  signed_in: 'success',
  signed_out: 'danger',
  unknown: 'default',
} as const satisfies Record<SignInState, ChipColor>

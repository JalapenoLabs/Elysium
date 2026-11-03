// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { BrowserConnectorKind, SignInState } from '../../../api/routes/connectorRoutes'

// Misc
import { BROWSER_CONNECTOR_KINDS } from '../../../api/routes/connectorRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

// Where each connector's service lives, so a browser on one of these hosts can feed it.
// Mirrors the reader origins in api/src/connectors/webmail.rs and slack.rs.
const hostsByConnectorKind = {
  outlook: [ 'outlook.office.com', 'outlook.office365.com', 'outlook.live.com' ],
  gmail: [ 'mail.google.com' ],
  slack: [ 'app.slack.com' ],
} as const satisfies Record<BrowserConnectorKind, readonly string[]>

// The connector that reads a browser started at `startUrl`, if one does.
export function connectorKindFor(startUrl: string): BrowserConnectorKind | null {
  if (!URL.canParse(startUrl)) {
    return null
  }
  const host = new URL(startUrl).hostname
  const kind = BROWSER_CONNECTOR_KINDS.find((candidate) => {
    const hosts: readonly string[] = hostsByConnectorKind[candidate]
    return hosts.includes(host)
  })
  return kind ?? null
}

export const connectorLabelKeys = {
  outlook: 'connectors.kinds.outlook',
  gmail: 'connectors.kinds.gmail',
  slack: 'connectors.kinds.slack',
} as const satisfies Record<BrowserConnectorKind, ParseKeys<'browsers'>>

// What the reading section says, by what the connector reads: mail, or messages.
type ReadingCopy = {
  title: ParseKeys<'browsers'>
  offer: ParseKeys<'browsers'>
  reading: ParseKeys<'browsers'>
  start: ParseKeys<'browsers'>
  started: ParseKeys<'browsers'>
  stopped: ParseKeys<'browsers'>
}

const mailCopy = {
  title: 'connectors.mail.title',
  offer: 'connectors.mail.offer',
  reading: 'connectors.mail.reading',
  start: 'connectors.mail.start',
  started: 'connectors.mail.started',
  stopped: 'connectors.mail.stopped',
} as const satisfies ReadingCopy

export const readingCopyByKind = {
  outlook: mailCopy,
  gmail: mailCopy,
  slack: {
    title: 'connectors.slack.title',
    offer: 'connectors.slack.offer',
    reading: 'connectors.slack.reading',
    start: 'connectors.slack.start',
    started: 'connectors.slack.started',
    stopped: 'connectors.slack.stopped',
  },
} as const satisfies Record<BrowserConnectorKind, ReadingCopy>

// What a connector started from its browser's page reads: a mailbox reads its whole inbox, and
// Slack starts with everything the browser is signed in to, narrowed later in the hub.
export const startingContentByKind = {
  outlook: {},
  gmail: {},
  slack: { conversations: '*' },
} as const satisfies Record<BrowserConnectorKind, Record<string, unknown>>

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

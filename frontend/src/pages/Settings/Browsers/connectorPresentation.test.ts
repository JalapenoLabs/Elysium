// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { BROWSER_CONNECTOR_KINDS } from '../../../api/routes/connectorRoutes'
import { connectorKindFor, startingContentByKind } from './connectorPresentation'

describe('connectorKindFor', () => {
  it('finds the connector for a browser started on its mailbox', () => {
    expect(connectorKindFor('https://outlook.office.com/mail/')).toBe('outlook')
    expect(connectorKindFor('https://outlook.live.com/mail/0/')).toBe('outlook')
    expect(connectorKindFor('https://mail.google.com/mail/')).toBe('gmail')
  })

  it('finds Slack for a browser started on its web client', () => {
    expect(connectorKindFor('https://app.slack.com/client')).toBe('slack')
    expect(connectorKindFor('https://mooreslab.slack.com/')).toBeNull()
  })

  it('finds none for any other page', () => {
    expect(connectorKindFor('https://example.com/')).toBeNull()
    expect(connectorKindFor('https://outlook.office.com.example.net/')).toBeNull()
    expect(connectorKindFor('not a url')).toBeNull()
  })
})

describe('startingContentByKind', () => {
  it('starts Slack on everything it is signed in to and a mailbox on its inbox', () => {
    expect(startingContentByKind.slack).toEqual({ conversations: '*' })
    expect(startingContentByKind.outlook).toEqual({})
    expect(Object.keys(startingContentByKind).sort()).toEqual([ ...BROWSER_CONNECTOR_KINDS ].sort())
  })
})

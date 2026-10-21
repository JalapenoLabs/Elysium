// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { connectorKindFor } from './connectorPresentation'

describe('connectorKindFor', () => {
  it('finds the connector for a browser started on its mailbox', () => {
    expect(connectorKindFor('https://outlook.office.com/mail/')).toBe('outlook')
    expect(connectorKindFor('https://outlook.live.com/mail/0/')).toBe('outlook')
    expect(connectorKindFor('https://mail.google.com/mail/')).toBe('gmail')
  })

  it('finds none for any other page', () => {
    expect(connectorKindFor('https://example.com/')).toBeNull()
    expect(connectorKindFor('https://outlook.office.com.example.net/')).toBeNull()
    expect(connectorKindFor('not a url')).toBeNull()
  })
})

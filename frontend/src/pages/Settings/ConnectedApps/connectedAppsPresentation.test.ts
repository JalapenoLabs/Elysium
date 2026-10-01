// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { connectCommandByClient, mcpServerName, mcpServerUrl } from './connectedAppsPresentation'

describe('mcpServerUrl', () => {
  it('hangs the MCP path off the page\'s origin', () => {
    expect(mcpServerUrl('https://work.example.com')).toBe('https://work.example.com/api/mcp')
  })
})

describe('mcpServerName', () => {
  it('lowercases the brand and keeps it safe for a shell', () => {
    expect(mcpServerName('Acme Works')).toBe('acme-works')
    expect(mcpServerName('  Mixed_Case! ')).toBe('mixed-case')
  })
})

describe('connectCommandByClient', () => {
  it('spells each client\'s add command', () => {
    const url = 'https://work.example.com/api/mcp'
    expect(connectCommandByClient.claudeCode('acme', url))
      .toBe('claude mcp add --transport http acme https://work.example.com/api/mcp')
    expect(connectCommandByClient.codex('acme', url))
      .toBe('codex mcp add acme --url https://work.example.com/api/mcp')
  })
})

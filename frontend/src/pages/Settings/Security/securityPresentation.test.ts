// Copyright © 2026 Jalapeno Labs

import type { KratosUi } from '../../../api/kratosFlows'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { readAuthenticator, readLookupCodes, readPasskeys, readProfile } from './securityPresentation'

function input(name: string, group: string, extra: Record<string, unknown> = {}, label?: unknown) {
  return {
    type: 'input',
    group,
    attributes: { node_type: 'input', name, type: 'submit', disabled: false, ...extra },
    messages: [],
    meta: label
      ? { label }
      : {},
  }
}

function text(id: string, group: string, value: unknown) {
  return { type: 'text', group, attributes: { node_type: 'text', id, text: value }, messages: [], meta: {}}
}

// Settings flows arrive as JSON shaped like these.
function flow(nodes: unknown[]) {
  return { id: 'settings-1', ui: { nodes }} as unknown as KratosUi
}

describe('readProfile', () => {
  it('reads the name and email Kratos holds', () => {
    const settings = flow([
      input('traits.email', 'profile', { value: 'ada@example.com', type: 'email' }),
      input('traits.name', 'profile', { value: 'Ada', type: 'text' }),
    ])
    expect(readProfile(settings)).toEqual({ name: 'Ada', email: 'ada@example.com' })
  })
})

describe('readPasskeys', () => {
  it('lists each passkey from its remove button', () => {
    const settings = flow([
      input('passkey_remove', 'passkey', { value: 'a1b2' }, {
        id: 1050018,
        text: 'Remove passkey "Bitwarden"',
        type: 'info',
        context: { display_name: 'Bitwarden', added_at: '2026-09-01T10:00:00Z' },
      }),
      input('passkey_remove', 'passkey', { value: 'c3d4', disabled: true }, {
        id: 1050018,
        text: 'Remove passkey "unnamed"',
        type: 'info',
        context: { display_name: 'unnamed' },
      }),
    ])
    expect(readPasskeys(settings)).toEqual([
      { id: 'a1b2', name: 'Bitwarden', addedAt: '2026-09-01T10:00:00Z', canRemove: true },
      { id: 'c3d4', name: '', addedAt: null, canRemove: false },
    ])
  })
})

describe('readAuthenticator', () => {
  it('is enrolled once Kratos offers to unlink it', () => {
    expect(readAuthenticator(flow([ input('totp_unlink', 'totp') ]))).toEqual({ kind: 'enrolled' })
  })

  it('offers the QR code and key to set one up', () => {
    const settings = flow([
      {
        type: 'img',
        group: 'totp',
        attributes: { node_type: 'img', id: 'totp_qr', src: 'data:image/png;base64,AA' },
        messages: [],
        meta: {},
      },
      text('totp_secret_key', 'totp', { id: 1050006, text: 'JBSWY3DPEHPK3PXP', type: 'info' }),
      input('totp_code', 'totp', { type: 'text' }),
    ])
    expect(readAuthenticator(settings)).toEqual({
      kind: 'available',
      qrCode: 'data:image/png;base64,AA',
      secretKey: 'JBSWY3DPEHPK3PXP',
    })
  })

  it('is unavailable when the flow offers neither', () => {
    expect(readAuthenticator(flow([]))).toEqual({ kind: 'unavailable' })
  })
})

describe('readLookupCodes', () => {
  it('shows fresh codes and marks the used ones', () => {
    const settings = flow([
      text('lookup_secret_codes', 'lookup_secret', {
        id: 1050015,
        text: 'abcd1234, used',
        type: 'info',
        context: {
          secrets: [
            { id: 1050009, text: 'abcd1234', type: 'info', context: { secret: 'abcd1234' }},
            { id: 1050014, text: 'Used', type: 'info', context: { used_at: '2026-09-02T00:00:00Z' }},
          ],
        },
      }),
      input('lookup_secret_confirm', 'lookup_secret'),
    ])
    expect(readLookupCodes(settings)).toEqual({
      codes: [{ code: 'abcd1234' }, { code: null }],
      canReveal: false,
      canRegenerate: false,
      canConfirm: true,
      canDisable: false,
    })
  })

  it('keeps codes hidden until they are revealed', () => {
    const settings = flow([
      input('lookup_secret_reveal', 'lookup_secret'),
      input('lookup_secret_regenerate', 'lookup_secret'),
      input('lookup_secret_disable', 'lookup_secret'),
    ])
    expect(readLookupCodes(settings)).toEqual({
      codes: null,
      canReveal: true,
      canRegenerate: true,
      canConfirm: false,
      canDisable: true,
    })
  })
})

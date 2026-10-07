// Copyright © 2026 Jalapeno Labs

import type { KratosUi } from '../../../api/kratosFlows'

// Misc
import { findInput } from '../../../api/kratosFlows'

// Reads what a Kratos settings flow says about each way of signing in. Kratos describes
// them as form nodes, named in its `ui/node/identifiers.go`; these turn them into what the
// Sign-in & security page shows.

// A text or image node, which Kratos names by `id` rather than `name`.
function findDisplayNode(flow: KratosUi, id: string) {
  for (const node of flow.ui.nodes) {
    const attributes = node.attributes
    if ((attributes.node_type === 'text' || attributes.node_type === 'img') && attributes.id === id) {
      return attributes
    }
  }
  return null
}

// One field of a Kratos message's `context`, which arrives as untyped JSON.
function readContext(context: unknown, key: string): unknown {
  if (typeof context !== 'object' || context === null) {
    return undefined
  }
  return Object.entries(context).find(([ entryKey ]) => entryKey === key)?.[1]
}

export type Passkey = {
  // What Kratos wants back in `passkey_remove` to remove it.
  id: string
  // Empty when the authenticator gave it no name.
  name: string
  addedAt: string | null
  // Kratos refuses to remove the last way someone can sign in.
  canRemove: boolean
}

// Each passkey the person has, from its remove button.
export function readPasskeys(flow: KratosUi): Passkey[] {
  const passkeys: Passkey[] = []
  for (const node of flow.ui.nodes) {
    const attributes = node.attributes
    if (attributes.node_type !== 'input' || attributes.name !== 'passkey_remove') {
      continue
    }
    const context = node.meta.label?.context
    // Kratos names a passkey "unnamed" when the authenticator gave it no name.
    const name = readContext(context, 'display_name')
    const addedAt = readContext(context, 'added_at')
    passkeys.push({
      id: String(attributes.value ?? ''),
      name: typeof name === 'string' && name !== 'unnamed'
        ? name
        : '',
      addedAt: typeof addedAt === 'string'
        ? addedAt
        : null,
      canRemove: !attributes.disabled,
    })
  }
  return passkeys
}

export type AuthenticatorState =
  // Set up; it can be removed.
  | { kind: 'enrolled' }
  // Not set up: scan this QR code, or type this key, then confirm with a code.
  | { kind: 'available', qrCode: string, secretKey: string }
  // The flow offers nothing, such as while it loads.
  | { kind: 'unavailable' }

export function readAuthenticator(flow: KratosUi): AuthenticatorState {
  if (findInput(flow, 'totp_unlink')) {
    return { kind: 'enrolled' }
  }

  const qr = findDisplayNode(flow, 'totp_qr')
  const secret = findDisplayNode(flow, 'totp_secret_key')
  if (qr?.node_type === 'img' && secret?.node_type === 'text') {
    return { kind: 'available', qrCode: qr.src, secretKey: secret.text.text }
  }
  return { kind: 'unavailable' }
}

export type LookupCode = {
  // The code, or null once it was used.
  code: string | null
}

export type LookupCodesState = {
  // Shown only right after revealing or regenerating them.
  codes: LookupCode[] | null
  canReveal: boolean
  canRegenerate: boolean
  // New codes wait for this before they replace the old ones.
  canConfirm: boolean
  canDisable: boolean
}

export function readLookupCodes(flow: KratosUi): LookupCodesState {
  const list = findDisplayNode(flow, 'lookup_secret_codes')
  let codes: LookupCode[] | null = null
  if (list?.node_type === 'text') {
    const secrets = readContext(list.text.context, 'secrets')
    codes = Array.isArray(secrets)
      ? secrets.map((secret: unknown) => {
        // A used code carries `used_at` in place of `secret`.
        const code = readContext(readContext(secret, 'context'), 'secret')
        return {
          code: typeof code === 'string'
            ? code
            : null,
        }
      })
      : []
  }

  return {
    codes,
    canReveal: Boolean(findInput(flow, 'lookup_secret_reveal')),
    canRegenerate: Boolean(findInput(flow, 'lookup_secret_regenerate')),
    canConfirm: Boolean(findInput(flow, 'lookup_secret_confirm')),
    canDisable: Boolean(findInput(flow, 'lookup_secret_disable')),
  }
}

// The profile Kratos holds, as the settings flow shows it.
export function readProfile(flow: KratosUi) {
  const name = findInput(flow, 'traits.name')?.attributes.value
  const email = findInput(flow, 'traits.email')?.attributes.value
  return {
    name: typeof name === 'string'
      ? name
      : '',
    email: typeof email === 'string'
      ? email
      : '',
  } as const
}

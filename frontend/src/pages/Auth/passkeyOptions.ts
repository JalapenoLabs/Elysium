// Copyright © 2026 Jalapeno Labs

import type {
  PublicKeyCredentialCreationOptionsJSON,
  PublicKeyCredentialRequestOptionsJSON,
} from '@simplewebauthn/browser'

// Kratos hands WebAuthn options to the browser as JSON in a hidden node: `passkey_challenge`
// to sign in, `passkey_create_data` to add a passkey. Both wrap the options the WebAuthn
// API takes in `publicKey`, with binary fields in base64url, which is the JSON form
// `@simplewebauthn/browser` reads.
type WrappedOptions<Options> = {
  publicKey: Options
}

function unwrap<Options>(value: string): Options {
  const parsed: WrappedOptions<Options> = JSON.parse(value)
  if (!parsed?.publicKey) {
    throw new Error('Kratos sent passkey options without publicKey')
  }
  return parsed.publicKey
}

export function readPasskeyRequestOptions(value: string) {
  return unwrap<PublicKeyCredentialRequestOptionsJSON>(value)
}

export function readPasskeyCreationOptions(value: string) {
  return unwrap<PublicKeyCredentialCreationOptionsJSON>(value)
}

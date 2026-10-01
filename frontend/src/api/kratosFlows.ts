// Copyright © 2026 Jalapeno Labs

import type { UiContainer, UiNode, UiNodeInputAttributes, UiText } from '@ory/client-fetch'

// Utility
import { ResponseError } from '@ory/client-fetch'

// What every Kratos flow carries: the form it wants, as nodes, and messages for the whole
// form. Login, registration, recovery, and settings flows all have this shape.
export type KratosUi = {
  id: string
  ui: UiContainer
}

// What a failed Kratos call means for the page that made it.
export type KratosFailure =
  // The submission was refused; the flow comes back with messages to show.
  | { kind: 'invalid', flow: KratosUi }
  // Changing a password, passkey, or authenticator needs a recent sign-in.
  | { kind: 'refresh' }
  // Signed in with one factor, and the account has a second to use.
  | { kind: 'secondFactor' }
  // Kratos wants the browser somewhere else, such as a new settings flow after recovery.
  | { kind: 'redirect', to: string }
  // The flow expired or its anti-forgery token no longer matches: start a new one.
  | { kind: 'expired' }
  // Someone is already signed in, so there is nothing to sign in to.
  | { kind: 'signedIn' }
  // Nobody is signed in, or the session ended, for a flow that needs one.
  | { kind: 'signedOut' }
  | { kind: 'failed', message: string | null }

// Kratos error ids, and what each means for a page.
const failureKindByErrorId = {
  session_refresh_required: 'refresh',
  session_aal2_required: 'secondFactor',
  self_service_flow_expired: 'expired',
  security_csrf_violation: 'expired',
  security_identity_mismatch: 'expired',
  session_already_available: 'signedIn',
  session_inactive: 'signedOut',
} as const satisfies Record<string, KratosFailure['kind']>

type KratosErrorBody = {
  ui?: UiContainer
  id?: string
  redirect_browser_to?: string
  error?: {
    id?: string
    message?: string
    reason?: string
  }
}

function isKnownErrorId(id: string): id is keyof typeof failureKindByErrorId {
  return id in failureKindByErrorId
}

// Reads what a failed Kratos call answered. Anything that is not an HTTP answer (a network
// failure) is `failed` with no message.
export async function readKratosFailure(error: unknown): Promise<KratosFailure> {
  if (!(error instanceof ResponseError)) {
    console.debug('A Kratos call failed without an answer', { error })
    return { kind: 'failed', message: null }
  }

  const body: KratosErrorBody | null = await error.response.clone().json().catch(() => null)
  return toKratosFailure(error.response.status, body)
}

// The decision `readKratosFailure` makes, apart from reading the body, so it can be tested.
export function toKratosFailure(status: number, body: KratosErrorBody | null): KratosFailure {
  if (body?.redirect_browser_to) {
    return { kind: 'redirect', to: body.redirect_browser_to }
  }

  const errorId = body?.error?.id
  if (errorId && isKnownErrorId(errorId)) {
    const kind = failureKindByErrorId[errorId]
    return { kind }
  }

  if (status === 400 && body?.ui && body.id) {
    return { kind: 'invalid', flow: { id: body.id, ui: body.ui }}
  }
  if (status === 410) {
    return { kind: 'expired' }
  }

  console.debug('Kratos answered with an error this page does not handle', { status, body })
  return { kind: 'failed', message: body?.error?.reason ?? body?.error?.message ?? null }
}

function inputAttributes(node: UiNode): UiNodeInputAttributes | null {
  if (node.attributes.node_type !== 'input') {
    return null
  }
  return node.attributes
}

// The input node named `name`, if the flow has one.
export function findInput(flow: KratosUi, name: string) {
  for (const node of flow.ui.nodes) {
    const attributes = inputAttributes(node)
    if (attributes?.name === name) {
      return { node, attributes } as const
    }
  }
  return null
}

// The value Kratos put in the input named `name`, as text.
export function inputValue(flow: KratosUi, name: string): string {
  const value: unknown = findInput(flow, name)?.attributes.value
  if (typeof value === 'string') {
    return value
  }
  return ''
}

// The anti-forgery token every submission of a browser flow must echo back.
export function csrfToken(flow: KratosUi): string {
  return inputValue(flow, 'csrf_token')
}

// Whether the flow offers a method at all, such as `passkey` or `totp`.
export function hasGroup(flow: KratosUi, group: string): boolean {
  return flow.ui.nodes.some((node) => node.group === group)
}

// Messages for the whole form, and for each field, in one pass over the nodes.
export function collectMessages(flow: KratosUi) {
  const byField: Record<string, UiText[]> = {}
  for (const node of flow.ui.nodes) {
    const attributes = inputAttributes(node)
    if (!attributes || !node.messages.length) {
      continue
    }
    byField[attributes.name] = [ ...(byField[attributes.name] ?? []), ...node.messages ]
  }
  return {
    form: flow.ui.messages ?? [],
    byField,
  } as const
}

// A path on this origin, for a `return_to` taken from the address. Anything else, such as
// another site, is dropped, so a crafted link can never send someone away after signing in.
//
// The value is resolved the way the browser will resolve it, and kept only if it lands on this
// origin. Checking the text instead misses forms the URL parser normalizes, such as `/\host`,
// which a browser reads as `//host`.
export function toLocalPath(value: string | null | undefined): string | null {
  if (!value?.startsWith('/')) {
    console.debug('toLocalPath dropped a return_to that is not a path', { value })
    return null
  }

  const resolved = new URL(value, window.location.origin)
  if (resolved.origin !== window.location.origin) {
    console.debug('toLocalPath dropped a return_to that leaves this origin', { value })
    return null
  }
  return `${resolved.pathname}${resolved.search}${resolved.hash}`
}

// Copyright © 2026 Jalapeno Labs

import type { KratosUi } from '../../../api/kratosFlows'

// Misc
import { findInput } from '../../../api/kratosFlows'

export type Profile = {
  firstName: string
  lastName: string
  email: string
}

function readText(flow: KratosUi, name: string) {
  const value = findInput(flow, name)?.attributes.value
  return typeof value === 'string'
    ? value
    : ''
}

// The profile Kratos holds, as a settings flow shows it. An identity the API has not yet
// moved to the current schema still has one name (`traits.name`), shown split at its first
// space, the way the API will split it.
export function readProfile(flow: KratosUi): Profile {
  const email = readText(flow, 'traits.email')
  if (findInput(flow, 'traits.name.first')) {
    return {
      firstName: readText(flow, 'traits.name.first'),
      lastName: readText(flow, 'traits.name.last'),
      email,
    }
  }

  const whole = readText(flow, 'traits.name').trim()
  const separator = whole.search(/\s/)
  if (separator === -1) {
    return { firstName: whole, lastName: '', email }
  }
  return {
    firstName: whole.slice(0, separator),
    lastName: whole.slice(separator).trim(),
    email,
  }
}

// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// A global variable every coding session's satellite thread receives. Mirrors
// `EnvironmentVariableResponse` in api/src/routes/v1/environment_variables/mod.rs.
export type EnvironmentVariable = {
  id: string
  key: string
  // A secret's value is never returned. The agent can still read it; secret only means
  // Elysium never shows it again and the satellite scrubs it from thread output.
  isSecret: boolean
  description: string
  // The plaintext of a non-secret variable; null for a secret one.
  value: string | null
  createdAt: string
  updatedAt: string
}

type ListEnvironmentVariablesResponse = {
  variables: EnvironmentVariable[]
}

export function listEnvironmentVariables() {
  return apiClient
    .get('v1/environment-variables')
    .json<ListEnvironmentVariablesResponse>()
}

type EnvironmentVariableResponse = {
  variable: EnvironmentVariable
}

type CreateEnvironmentVariableRequest = {
  key: string
  value: string
  isSecret: boolean
  description?: string
}

export function createEnvironmentVariable(body: CreateEnvironmentVariableRequest) {
  return apiClient
    .post('v1/environment-variables', { json: body })
    .json<EnvironmentVariableResponse>()
}

// Omitted fields are left unchanged, and an omitted value keeps the sealed one. Making a
// secret variable visible needs its value sent again.
type UpdateEnvironmentVariableRequest = {
  key?: string
  value?: string
  isSecret?: boolean
  description?: string
}

export function updateEnvironmentVariable(variableId: string, body: UpdateEnvironmentVariableRequest) {
  return apiClient
    .patch(`v1/environment-variables/${variableId}`, { json: body })
    .json<EnvironmentVariableResponse>()
}

export function deleteEnvironmentVariable(variableId: string) {
  return apiClient
    .delete(`v1/environment-variables/${variableId}`)
}

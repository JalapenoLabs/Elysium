// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// What a user may do. People are admins, members, or guests; machines are agents or the
// system. A person waiting for approval has none.
export type UserRole = 'admin' | 'member' | 'guest' | 'agent' | 'system'

// The roles a person can be given.
export type PersonRole = 'admin' | 'member' | 'guest'

export type UserStatus = 'pending' | 'active' | 'disabled'

// Mirrors `UserResponse` in api/src/routes/v1/users/mod.rs. Machines have no email and no
// status.
export type User = {
  id: string
  kind: 'person' | 'machine'
  name: string
  email: string | null
  role: UserRole | null
  status: UserStatus | null
  approvedAt: string | null
  lastSeenAt: string | null
  createdAt: string
  updatedAt: string
  // How the person signs in (`password`, `passkey`, `totp`, `lookup_secret`). Sent to admins
  // only, and left out when Kratos could not be asked.
  signInMethods?: string[]
}

type ListUsersResponse = {
  users: User[]
}

export function listUsers() {
  return apiClient
    .get('v1/users')
    .json<ListUsersResponse>()
}

type UserResponse = {
  user: User
}

export function approveUser(userId: string, role: PersonRole) {
  return apiClient
    .post(`v1/users/${userId}/approve`, { json: { role }})
    .json<UserResponse>()
}

// Refuses a pending sign-up: the account and its sign-in are deleted.
export function rejectUser(userId: string) {
  return apiClient
    .delete(`v1/users/${userId}`)
}

// Omitted fields are left unchanged. Answers 409 when it would leave no active admin.
type UpdateUserRequest = {
  role?: PersonRole
  disabled?: boolean
}

export function updateUser(userId: string, body: UpdateUserRequest) {
  return apiClient
    .patch(`v1/users/${userId}`, { json: body })
    .json<UserResponse>()
}

export function revokeUserSessions(userId: string) {
  return apiClient
    .post(`v1/users/${userId}/revoke-sessions`)
}

export function resetUserMfa(userId: string) {
  return apiClient
    .post(`v1/users/${userId}/reset-mfa`)
}

type RecoveryLinkResponse = {
  recoveryLink: string
  expiresAt: string
}

export function createUserRecoveryLink(userId: string) {
  return apiClient
    .post(`v1/users/${userId}/recovery-link`)
    .json<RecoveryLinkResponse>()
}

// Mirrors `WorkspaceSettingsResponse` in api/src/routes/v1/workspace_settings/mod.rs.
export type WorkspaceSettings = {
  // Whether anyone may sign up. The first person may sign up either way.
  signupOpen: boolean
  // Whether every person must set up an authenticator app.
  requireMfa: boolean
  updatedBy: string
  updatedAt: string
}

type WorkspaceSettingsResponse = {
  settings: WorkspaceSettings
}

export function getWorkspaceSettings() {
  return apiClient
    .get('v1/workspace-settings')
    .json<WorkspaceSettingsResponse>()
}

type UpdateWorkspaceSettingsRequest = {
  signupOpen?: boolean
  requireMfa?: boolean
}

export function updateWorkspaceSettings(body: UpdateWorkspaceSettingsRequest) {
  return apiClient
    .patch('v1/workspace-settings', { json: body })
    .json<WorkspaceSettingsResponse>()
}

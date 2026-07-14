// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

export type Team = {
  id: string
  name: string
  slug: string
}

export type TeamMember = {
  user_id: string
  email: string
  display_name: string
  avatar_url: string
  role: string
}

// Teams within an org: any member views, admins and owners mutate (the server
// enforces org RBAC; a 403 is the caller's cue for the denied surface).

export function listTeams(orgId: string): Promise<{ teams: Team[] }> {
  return apiClient
    .get(`api/v1/orgs/${orgId}/teams`)
    .json<{ teams: Team[] }>()
}

export function createTeam(orgId: string, name: string): Promise<Team> {
  return apiClient
    .post(`api/v1/orgs/${orgId}/teams`, { json: { name }})
    .json<Team>()
}

export function renameTeam(teamId: string, name: string): Promise<Response> {
  return apiClient.patch(`api/v1/teams/${teamId}`, { json: { name }})
}

export function deleteTeam(teamId: string): Promise<Response> {
  return apiClient.delete(`api/v1/teams/${teamId}`)
}

export function listTeamMembers(teamId: string): Promise<{ members: TeamMember[] }> {
  return apiClient
    .get(`api/v1/teams/${teamId}/members`)
    .json<{ members: TeamMember[] }>()
}

// The target must already be an org member.
export function addTeamMember(teamId: string, userId: string): Promise<Response> {
  return apiClient.post(`api/v1/teams/${teamId}/members`, {
    json: { user_id: userId },
  })
}

export function removeTeamMember(teamId: string, userId: string): Promise<Response> {
  return apiClient.delete(`api/v1/teams/${teamId}/members/${userId}`)
}

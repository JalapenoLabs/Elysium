// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Jira's own ids for a project and a board, passed through untouched: the pickers key on
// them, and nothing here parses or renumbers one. Both are spelled as strings over the
// API, so a board id Jira reports as a number arrives already converted.
export type JiraProjectId = string
export type JiraBoardId = string

// A project the token can reach, as Jira reports it.
export type JiraProject = {
  projectId: JiraProjectId
  // The short key Jira puts in front of every issue, such as ELY.
  projectKey: string
  name: string
}

// A board the token can reach. Boards live in Jira's Agile API, alongside projects
// rather than inside them, so a board is allowed on its own.
export type JiraBoard = {
  boardId: JiraBoardId
  name: string
  // The project the board draws from; null for a board that spans several.
  projectKey: string | null
}

// The Jira account a token turned out to belong to.
export type JiraAccount = {
  accountId: string
  displayName: string
  email: string
}

// How an allowlist is sent: `*` for everything the token can reach, including projects
// and boards added later, or the ids picked one by one.
export const ALL_JIRA_ITEMS = '*'
export type JiraScope = typeof ALL_JIRA_ITEMS | string[]

// The API never returns the token. The account, and the projects and boards below it,
// are what Jira answered when the token was last checked, which happens on every save
// and every test.
export type JiraCredential = {
  id: string
  name: string
  // https://<site>.atlassian.net
  siteUrl: string
  accountEmail: string
  accountId: string
  displayName: string
  // True when every project the token reaches is allowed, including ones added later.
  allProjects: boolean
  // The projects allowed one by one; empty while `allProjects` is true.
  projects: JiraProject[]
  allBoards: boolean
  boards: JiraBoard[]
  checkedAt: string
  createdAt: string
  updatedAt: string
}

type ListJiraCredentialsResponse = {
  credentials: JiraCredential[]
}

export function listJiraCredentials() {
  return apiClient
    .get('v1/jira-credentials')
    .json<ListJiraCredentialsResponse>()
}

type DiscoverJiraRequest = {
  siteUrl: string
  accountEmail: string
  token: string
}

export type JiraDiscovery = {
  account: JiraAccount
  projects: JiraProject[]
  boards: JiraBoard[]
}

// Checks a token with Jira and answers the account it belongs to along with every
// project and board it can reach. Nothing is stored: the credential is created in a
// second call, with what was picked from these lists. A token Jira refuses answers 400.
export function discoverJira(body: DiscoverJiraRequest) {
  return apiClient
    .post('v1/jira-credentials/discover', { json: body })
    .json<JiraDiscovery>()
}

type JiraCredentialResponse = {
  credential: JiraCredential
}

type CreateJiraCredentialRequest = {
  name: string
  siteUrl: string
  accountEmail: string
  token: string
  projects: JiraScope
  boards: JiraScope
}

export function createJiraCredential(body: CreateJiraCredentialRequest) {
  return apiClient
    .post('v1/jira-credentials', { json: body })
    .json<JiraCredentialResponse>()
}

// Omitted fields are left unchanged. A stored token belongs to one site and one account,
// so a different site or email must arrive with the token that goes with it.
type UpdateJiraCredentialRequest = {
  name?: string
  siteUrl?: string
  accountEmail?: string
  token?: string
  projects?: JiraScope
  boards?: JiraScope
}

export function updateJiraCredential(credentialId: string, body: UpdateJiraCredentialRequest) {
  return apiClient
    .patch(`v1/jira-credentials/${credentialId}`, { json: body })
    .json<JiraCredentialResponse>()
}

export function deleteJiraCredential(credentialId: string) {
  return apiClient
    .delete(`v1/jira-credentials/${credentialId}`)
}

type TestJiraCredentialResponse = {
  result: {
    account: JiraAccount
    // Allowed projects and boards the stored token can no longer reach, which is how a
    // revoked grant shows up. Absent when the API reports none.
    unreachableProjects?: JiraProject[]
    unreachableBoards?: JiraBoard[]
  }
}

// Asks Jira about the stored token right now. A token Jira refuses answers 400, and an
// unreachable Jira 502.
export function testJiraCredential(credentialId: string) {
  return apiClient
    .post(`v1/jira-credentials/${credentialId}/test`)
    .json<TestJiraCredentialResponse>()
}

// What the token can reach today, each marked with whether the credential allows it.
export type JiraProjectOption = JiraProject & {
  selected: boolean
}

export type JiraBoardOption = JiraBoard & {
  selected: boolean
}

type ListJiraProjectsResponse = {
  projects: JiraProjectOption[]
}

export function listJiraCredentialProjects(credentialId: string) {
  return apiClient
    .get(`v1/jira-credentials/${credentialId}/projects`)
    .json<ListJiraProjectsResponse>()
}

type ListJiraBoardsResponse = {
  boards: JiraBoardOption[]
}

export function listJiraCredentialBoards(credentialId: string) {
  return apiClient
    .get(`v1/jira-credentials/${credentialId}/boards`)
    .json<ListJiraBoardsResponse>()
}

// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// A project the token can reach, as Jira reports it. Mirrors `docs/jira.md`.
export type JiraProject = {
  id: string
  // The short key Jira puts in front of every issue, such as ELY.
  key: string
  name: string
}

// A board the token can reach. Boards live in Jira's Agile API alongside projects rather
// than inside them, so a board is allowed on its own. Its id is a number, unlike a
// project's.
export type JiraBoard = {
  id: number
  name: string
  // The project the board draws from; null for a board that spans no single one.
  projectKey: string | null
}

// The Jira account a token turned out to belong to.
export type JiraAccount = {
  accountId: string
  displayName: string
  // Null when the account hides its address, which Atlassian allows. The address the
  // client signs in with is the credential's own `accountEmail`.
  email: string | null
}

// An allowlist of everything, including projects and boards added later.
export const ALL_JIRA_ITEMS = '*'

// What the API returns: everything, or the entries the credential may touch, each stored
// with the id, key, and name Jira reported.
export type JiraProjectScope = typeof ALL_JIRA_ITEMS | JiraProject[]
export type JiraBoardScope = typeof ALL_JIRA_ITEMS | JiraBoard[]

// What a write sends: everything, or the ids picked one by one. A project is named by its
// id, a board by its numeric id.
export type JiraProjectSelection = typeof ALL_JIRA_ITEMS | string[]
export type JiraBoardSelection = typeof ALL_JIRA_ITEMS | number[]

// The API never returns the token. The account is what Jira answered when the token was
// last checked, which happens on every save and every test.
export type JiraCredential = {
  id: string
  name: string
  // The site's origin, `https://<site>.atlassian.net`, normalized by the API.
  siteUrl: string
  accountEmail: string
  accountId: string
  displayName: string
  projects: JiraProjectScope
  boards: JiraBoardScope
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
  // True when Jira held more pages than Elysium reads.
  projectsTruncated: boolean
  boardsTruncated: boolean
}

// Checks a token with Jira and answers the account it belongs to along with every project
// and board it can reach. Nothing is stored: the credential is created in a second call,
// with what was picked from these lists. A token Jira refuses answers 400.
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
  projects: JiraProjectSelection
  boards: JiraBoardSelection
}

// A selection Jira does not report for that token answers 400 naming it, and a name
// already taken answers 409.
export function createJiraCredential(body: CreateJiraCredentialRequest) {
  return apiClient
    .post('v1/jira-credentials', { json: body })
    .json<JiraCredentialResponse>()
}

// Omitted fields are left unchanged. A token belongs to one account on one site, so a
// different site or email must arrive with the token; a different site must also bring
// both allowlists, since a selection made on the old site means nothing on the new one.
type UpdateJiraCredentialRequest = {
  name?: string
  siteUrl?: string
  accountEmail?: string
  token?: string
  projects?: JiraProjectSelection
  boards?: JiraBoardSelection
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

// A stored selection the token has lost access to comes back `reachable: false` rather
// than being dropped, so a page can say which it is.
export type JiraProjectReach = JiraProject & {
  reachable: boolean
}

export type JiraBoardReach = JiraBoard & {
  reachable: boolean
}

type TestJiraCredentialResponse = {
  // The credential as the test left it, with a fresh account and `checkedAt`.
  credential: JiraCredential
  result: {
    account: JiraAccount
    // Empty for a credential that allows everything: there is nothing stored to check.
    projects: JiraProjectReach[]
    boards: JiraBoardReach[]
  }
}

// Asks Jira about the stored token right now. A token Jira refuses answers 400, and an
// unreachable Jira 502.
export function testJiraCredential(credentialId: string) {
  return apiClient
    .post(`v1/jira-credentials/${credentialId}/test`)
    .json<TestJiraCredentialResponse>()
}

// What the token reaches today, each marked with whether the credential allows it.
// `selected` is true for every entry while the credential allows everything.
export type JiraProjectOption = JiraProject & {
  selected: boolean
}

export type JiraBoardOption = JiraBoard & {
  selected: boolean
}

type ListJiraProjectsResponse = {
  projects: JiraProjectOption[]
  truncated: boolean
}

export function listJiraCredentialProjects(credentialId: string) {
  return apiClient
    .get(`v1/jira-credentials/${credentialId}/projects`)
    .json<ListJiraProjectsResponse>()
}

type ListJiraBoardsResponse = {
  boards: JiraBoardOption[]
  truncated: boolean
}

export function listJiraCredentialBoards(credentialId: string) {
  return apiClient
    .get(`v1/jira-credentials/${credentialId}/boards`)
    .json<ListJiraBoardsResponse>()
}

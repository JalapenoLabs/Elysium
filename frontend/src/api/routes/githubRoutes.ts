// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `GithubTokenKind` in api/src/models/github_credential.rs.
export const GITHUB_TOKEN_KINDS = [ 'classic', 'fine-grained' ] as const
export type GithubTokenKind = typeof GITHUB_TOKEN_KINDS[number]

// The API never returns the token. Every field below the name is what GitHub answered
// when the token was last checked, which happens on every save and every test.
export type GithubCredential = {
  id: string
  name: string
  kind: GithubTokenKind
  // The account the token acts as.
  login: string
  // A classic token's scopes; empty for a fine-grained token, whose permissions are per
  // repository and are not reported over the API.
  scopes: string[]
  // Null for a token that does not expire.
  tokenExpiresAt: string | null
  checkedAt: string
  // The workspace default, which sessions start with unless their project or the session
  // chooses otherwise.
  isDefault: boolean
  createdAt: string
  updatedAt: string
}

type ListGithubCredentialsResponse = {
  credentials: GithubCredential[]
}

export function listGithubCredentials() {
  return apiClient
    .get('v1/github-credentials')
    .json<ListGithubCredentialsResponse>()
}

type GithubCredentialResponse = {
  credential: GithubCredential
}

type CreateGithubCredentialRequest = {
  name: string
  kind: GithubTokenKind
  token: string
}

export function createGithubCredential(body: CreateGithubCredentialRequest) {
  return apiClient
    .post('v1/github-credentials', { json: body })
    .json<GithubCredentialResponse>()
}

// Omitted fields are left unchanged. A new kind of token must arrive with the token.
type UpdateGithubCredentialRequest = {
  name?: string
  kind?: GithubTokenKind
  token?: string
  // True replaces the workspace default with this token; false leaves no default.
  isDefault?: boolean
}

export function updateGithubCredential(credentialId: string, body: UpdateGithubCredentialRequest) {
  return apiClient
    .patch(`v1/github-credentials/${credentialId}`, { json: body })
    .json<GithubCredentialResponse>()
}

export function deleteGithubCredential(credentialId: string) {
  return apiClient
    .delete(`v1/github-credentials/${credentialId}`)
}

type TestGithubCredentialResponse = {
  result: {
    login: string
    scopes: string[]
    tokenExpiresAt: string | null
  }
}

export function testGithubCredential(credentialId: string) {
  return apiClient
    .post(`v1/github-credentials/${credentialId}/test`)
    .json<TestGithubCredentialResponse>()
}

export type RepositoryAccess = {
  // owner/name, as GitHub spells it.
  repository: string
  canRead: boolean
  // Null when it cannot be known, which is always the case for a fine-grained token.
  canPush: boolean | null
  // Null when the token cannot see the repository.
  isPrivate: boolean | null
}

type CheckRepositoryAccessResponse = {
  result: RepositoryAccess
}

// What a token can do with a repository on github.com. A token GitHub refuses answers 400.
export function checkRepositoryAccess(credentialId: string, repositoryUrl: string) {
  return apiClient
    .post(`v1/github-credentials/${credentialId}/repository-access`, { json: { repositoryUrl }})
    .json<CheckRepositoryAccessResponse>()
}

// Mirrors `RepositoryResponse` in api/src/routes/v1/github_credentials/list_repositories.rs.
export type GithubRepository = {
  // owner/name, as GitHub spells it.
  fullName: string
  owner: string
  name: string
  private: boolean
  archived: boolean
  defaultBranch: string
  // The https://github.com/ URL to clone from.
  cloneUrl: string
  // Null for a repository nothing was ever pushed to.
  pushedAt: string | null
  // Null when it cannot be known, which is always the case for a fine-grained token.
  canPush: boolean | null
}

type ListGithubRepositoriesResponse = {
  // Most recently pushed first.
  repositories: GithubRepository[]
  // True when the token sees more repositories than the API lists.
  truncated: boolean
}

// The repositories a token can see. A token GitHub refuses answers 400.
export function listGithubRepositories(credentialId: string) {
  return apiClient
    .get(`v1/github-credentials/${credentialId}/repositories`)
    .json<ListGithubRepositoriesResponse>()
}

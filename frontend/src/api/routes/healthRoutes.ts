// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

export function getOk() {
  return apiClient
    .get('ok')
    .text()
}

export function getPing() {
  return apiClient
    .get('ping')
    .text()
}

// Mirrors the `VersionInfo` struct in api/src/version.rs.
export type GitCommit = {
  hash: string
  shortHash: string
  author: string
  date: string
  subject: string
}

export type GetVersionResponse = {
  name: string
  version: string
  profile: string
  rustc: string
  builtAt: string
  git: {
    branch: string
    commit: GitCommit | null
    dirty: boolean
    history: GitCommit[]
  }
}

export function getVersion() {
  return apiClient
    .get('version')
    .json<GetVersionResponse>()
}

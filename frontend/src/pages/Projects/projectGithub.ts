// Copyright © 2026 Jalapeno Labs

import type { ProjectGithub } from '../../api/routes/projectRoutes'

// Misc
import { INHERIT_GITHUB_TOKEN, NO_GITHUB_TOKEN } from '../../components/GithubTokenSelect'

// What the token picker's two fixed choices save as. Every other key is a token id.
const githubByKey: Record<string, ProjectGithub> = {
  [INHERIT_GITHUB_TOKEN]: { access: 'default', credentialId: null },
  [NO_GITHUB_TOKEN]: { access: 'none', credentialId: null },
}

// The picker's key for a project's choice. A project whose token was deleted shows as
// following the default, which is what its sessions do.
export function toGithubKey(github: ProjectGithub) {
  if (github.access === 'none') {
    return NO_GITHUB_TOKEN
  }
  if (github.access === 'specific' && github.credentialId) {
    return github.credentialId
  }
  return INHERIT_GITHUB_TOKEN
}

export function fromGithubKey(key: string): ProjectGithub {
  return githubByKey[key] ?? { access: 'specific', credentialId: key }
}

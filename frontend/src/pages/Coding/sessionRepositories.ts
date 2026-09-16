// Copyright © 2026 Jalapeno Labs

// Misc
import { parseGithubRepository } from '../Settings/Github/githubPresentation'

// The most repositories one session clones, mirroring `MAX_REPOSITORIES` in
// api/src/routes/v1/coding_sessions/create_coding_session.rs.
export const MAX_SESSION_REPOSITORIES = 16

// The directory a repository is cloned into: the URL's last segment without `.git`, mirroring
// `repository_directory` in the same file. Null for a URL that names no plain directory.
export function getRepositoryDirectory(url: string) {
  const lastSegment = url.replace(/\/+$/, '').split(/[/:]/).pop() ?? ''
  const name = lastSegment.endsWith('.git')
    ? lastSegment.slice(0, -'.git'.length)
    : lastSegment

  if (!name || name.startsWith('.') || !/^[\w.-]+$/.test(name)) {
    return null
  }
  return name
}

// What makes two URLs the same repository, mirroring `repository_identity` in the API.
// github.com names are case-insensitive and reachable over HTTPS and SSH alike; any other host
// is compared as written, without a trailing slash or `.git`.
export function getRepositoryIdentity(url: string) {
  const githubRepository = parseGithubRepository(url)
  if (githubRepository) {
    return `github.com/${githubRepository}`.toLowerCase()
  }
  return url.replace(/\/+$/, '').replace(/\.git$/, '')
}

// Where the chosen token's repository list stands: no token to list with, still loading,
// refused or unreachable, or loaded.
export type RepositoryListingStatus = 'no-token' | 'loading' | 'failed' | 'loaded'

export type RepositoryConflict = {
  kind: 'duplicate' | 'directory'
  first: string
  second: string
  directory: string
}

// The first pair of URLs that would clone into the same directory, ignoring case, as the API
// refuses them (`refuse_shared_directories`). Null when every directory is its own.
export function findRepositoryConflict(urls: string[]): RepositoryConflict | null {
  const claimed = new Map<string, string>()

  for (const url of urls) {
    const directory = getRepositoryDirectory(url)
    if (!directory) {
      // The URL's own validation reports it; it claims no directory.
      continue
    }

    const earlier = claimed.get(directory.toLowerCase())
    if (earlier === undefined) {
      claimed.set(directory.toLowerCase(), url)
      continue
    }

    const kind = getRepositoryIdentity(earlier) === getRepositoryIdentity(url)
      ? 'duplicate'
      : 'directory'
    return { kind, first: earlier, second: url, directory }
  }

  return null
}

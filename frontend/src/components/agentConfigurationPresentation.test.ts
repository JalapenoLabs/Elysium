// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { commitUrl, shortCommit } from './agentConfigurationPresentation'

const COMMIT = '611c46a583443c3a8dd4caad4a89dffa964add43'

describe('commitUrl', () => {
  it('links a github.com repository to the commit, with or without .git', () => {
    expect(commitUrl('https://github.com/navarrotech/agents', COMMIT))
      .toBe(`https://github.com/navarrotech/agents/commit/${COMMIT}`)
    expect(commitUrl('https://github.com/navarrotech/agents.git', COMMIT))
      .toBe(`https://github.com/navarrotech/agents/commit/${COMMIT}`)
    expect(commitUrl('https://github.com/navarrotech/agents/', COMMIT))
      .toBe(`https://github.com/navarrotech/agents/commit/${COMMIT}`)
  })

  it('links nothing on another host, whose pages it does not know', () => {
    expect(commitUrl('https://git.example.com/acme/agents.git', COMMIT)).toBeNull()
  })

  it('links nothing for a repository that is not a URL', () => {
    expect(commitUrl('not a url', COMMIT)).toBeNull()
  })
})

describe('shortCommit', () => {
  it('abbreviates a commit as GitHub does', () => {
    expect(shortCommit(COMMIT)).toBe('611c46a')
  })
})

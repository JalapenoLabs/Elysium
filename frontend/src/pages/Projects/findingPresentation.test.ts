// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { findingCostOrder, repositoryHref } from './findingPresentation'

describe('repositoryHref', () => {
  it('opens a web address without its .git', () => {
    expect(repositoryHref({ repositoryUrl: 'https://github.com/JalapenoLabs/Elysium.git' }))
      .toBe('https://github.com/JalapenoLabs/Elysium')
    expect(repositoryHref({ repositoryUrl: 'https://gitlab.example.com/team/api' }))
      .toBe('https://gitlab.example.com/team/api')
  })

  it('opens nothing for a remote only Git reads, or no repository', () => {
    expect(repositoryHref({ repositoryUrl: 'git@github.com:JalapenoLabs/Elysium.git' })).toBeNull()
    expect(repositoryHref({ repositoryUrl: null })).toBeNull()
  })
})

describe('findingCostOrder', () => {
  it('sorts small before medium before large', () => {
    expect(findingCostOrder.small).toBeLessThan(findingCostOrder.medium)
    expect(findingCostOrder.medium).toBeLessThan(findingCostOrder.large)
  })
})

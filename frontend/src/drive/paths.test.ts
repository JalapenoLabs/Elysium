// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { joinPath, parentPath, pathSegments } from './paths'

describe('joinPath', () => {
  it('joins against the root without a leading slash', () => {
    expect(joinPath('', 'docs')).toBe('docs')
    expect(joinPath('docs', 'q3')).toBe('docs/q3')
    expect(joinPath('docs/q3', 'plan.md')).toBe('docs/q3/plan.md')
  })
})

describe('parentPath', () => {
  it('walks up one level, landing on the root', () => {
    expect(parentPath('docs/q3/plan.md')).toBe('docs/q3')
    expect(parentPath('docs')).toBe('')
    expect(parentPath('')).toBe('')
  })
})

describe('pathSegments', () => {
  it('returns breadcrumbs with cumulative paths', () => {
    expect(pathSegments('docs/q3')).toEqual([
      { name: 'docs', path: 'docs' },
      { name: 'q3', path: 'docs/q3' },
    ])
  })

  it('returns no segments for the root', () => {
    expect(pathSegments('')).toEqual([])
  })
})

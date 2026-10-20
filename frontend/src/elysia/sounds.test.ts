// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { soundForTransition } from './sounds'

describe('soundForTransition', () => {
  it('stays quiet for the first light after a page loads', () => {
    expect(soundForTransition(null, 'needs-you')).toBeNull()
    expect(soundForTransition(null, 'errored')).toBeNull()
  })

  it('stays quiet when nothing changed', () => {
    expect(soundForTransition('needs-you', 'needs-you')).toBeNull()
  })

  it('asks for attention when she needs a person, from any other light', () => {
    expect(soundForTransition('running', 'needs-you')).toBe('attention')
    expect(soundForTransition('done', 'needs-you')).toBe('attention')
  })

  it('sounds a failure when a turn fails', () => {
    expect(soundForTransition('running', 'errored')).toBe('failure')
  })

  it('chimes completion only when running work finishes', () => {
    expect(soundForTransition('running', 'done')).toBe('completion')
    expect(soundForTransition('needs-you', 'done')).toBeNull()
    expect(soundForTransition('errored', 'done')).toBeNull()
  })

  it('says nothing when she starts running', () => {
    expect(soundForTransition('done', 'running')).toBeNull()
  })
})

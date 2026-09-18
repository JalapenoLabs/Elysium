// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { DEFAULT_LOCALE } from '../../i18n'
import { createSessionFormSchema } from './createSessionFormSchema'

// testSetup initializes the app's i18next instance with the en-US translations.
const t = i18next.getFixedT(DEFAULT_LOCALE, 'coding')

const VALUES = {
  projectId: 'project',
  satelliteId: 'satellite',
  title: '',
  repositories: [],
  prompt: '',
  githubChoice: 'inherit',
}

describe('createSessionFormSchema', () => {
  it('lets a session of its own start without a prompt', () => {
    const schema = createSessionFormSchema(t, { isPromptRequired: false })
    expect(schema.safeParse(VALUES).success).toBe(true)
  })

  it('asks a session started from an item for a prompt that is not blank', () => {
    const schema = createSessionFormSchema(t, { isPromptRequired: true })

    const blank = schema.safeParse({ ...VALUES, prompt: '  \n ' })
    expect(blank.success).toBe(false)
    expect(blank.error?.issues[0]?.message).toBe('Say what the agent should do about the item.')

    expect(schema.safeParse({ ...VALUES, prompt: 'Fix it.' }).success).toBe(true)
  })
})

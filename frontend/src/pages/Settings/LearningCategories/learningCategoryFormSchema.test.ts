// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { createLearningCategoryFormSchema, KEY_MAX_CHARACTERS, keyFromLabel } from './learningCategoryFormSchema'

describe('keyFromLabel', () => {
  it('joins a label\'s words with underscores, as a key is written', () => {
    expect(keyFromLabel('EDA tools')).toBe('eda_tools')
    expect(keyFromLabel('  Skills & agents ')).toBe('skills_agents')
    expect(keyFromLabel('Client engagement')).toBe('client_engagement')
    expect(keyFromLabel('SoC')).toBe('soc')
  })

  it('keeps accented letters whole rather than splitting the word', () => {
    expect(keyFromLabel('Naïve café')).toBe('naive_cafe')
  })

  it('starts with a letter and stays within the longest key', () => {
    expect(keyFromLabel('2024 roadmap')).toBe('roadmap')
    expect(keyFromLabel('!!!')).toBe('')
    const long = keyFromLabel('word '.repeat(20))
    expect(long.length).toBeLessThanOrEqual(KEY_MAX_CHARACTERS)
    expect(long).not.toMatch(/_$/u)
  })
})

describe('createLearningCategoryFormSchema', () => {
  const schema = createLearningCategoryFormSchema(i18next.getFixedT('en-US', 'learningCategories'))
  const valid = {
    label: ' EDA tools ',
    key: 'eda_tools',
    description: ' Simulators, synthesis, and their licenses. ',
  }

  it('trims what it is given, as the API does', () => {
    expect(schema.parse(valid)).toEqual({
      label: 'EDA tools',
      key: 'eda_tools',
      description: 'Simulators, synthesis, and their licenses.',
    })
  })

  it('refuses a key a learning could not store, and blank text', () => {
    for (const key of [ '', 'EDA', 'eda-tools', '2fa', 'k'.repeat(KEY_MAX_CHARACTERS + 1) ]) {
      expect(schema.safeParse({ ...valid, key }).success).toBe(false)
    }
    expect(schema.safeParse({ ...valid, label: '  ' }).success).toBe(false)
    expect(schema.safeParse({ ...valid, description: '' }).success).toBe(false)
  })
})

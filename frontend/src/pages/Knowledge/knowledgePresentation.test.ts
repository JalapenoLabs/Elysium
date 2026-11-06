// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// User interface
import { LuFileText, LuLightbulb } from 'react-icons/lu'

// Misc
import { excerptPassage, KNOWLEDGE_SOURCES, presentKnowledgeSource, withoutTitle } from './knowledgePresentation'

describe('excerptPassage', () => {
  const longLine = `${'Some words that lead up to it. '.repeat(10)}Restart the fleet here.`

  it('leaves a passage whose first lines hold the match as it is', () => {
    expect(excerptPassage('Restart the fleet.\n\nThen wait.', 'restart')).toBe('Restart the fleet.\n\nThen wait.')
  })

  it('starts at the line holding the most of what was searched for', () => {
    const text = 'Section 1\n\nThe first point.\n\nThe second point of section 30.\n\nThe last.'

    expect(excerptPassage(text, 'second point section 30'))
      .toBe('… The second point of section 30.\n\nThe last.')
  })

  it('starts a long line shortly before its match, on a word', () => {
    const excerpt = excerptPassage(longLine, 'fleet')

    expect(excerpt.startsWith('… ')).toBe(true)
    expect(excerpt.endsWith('Restart the fleet here.')).toBe(true)
    expect(excerpt.length).toBeLessThan(90)
  })

  it('reads a passage found by its meaning from its start', () => {
    expect(excerptPassage('Nothing typed is here.', 'zeppelin')).toBe('Nothing typed is here.')
  })
})

describe('presentKnowledgeSource', () => {
  const t = i18next.getFixedT('en-US', 'knowledge')

  it('names every source the filters offer in this app\'s words', () => {
    for (const source of KNOWLEDGE_SOURCES) {
      const presented = presentKnowledgeSource(source, t)
      expect(presented.label).not.toMatch(/^sources\./)
    }
    expect(presentKnowledgeSource('learning', t)).toEqual({
      label: 'Learnings',
      icon: LuLightbulb,
      className: '',
    })
  })

  it('reads a source this build does not know as its own name', () => {
    expect(presentKnowledgeSource('jira', t).label).toBe('jira')
  })
})

describe('withoutTitle', () => {
  it('leaves the title out of the first passage, which the page shows above it', () => {
    const first = { position: 0, content: '#web\n\nFrom Maya Chen\n\nWAY-123 is the sign-in loop.' }

    expect(withoutTitle(first, '#web')).toBe('From Maya Chen\n\nWAY-123 is the sign-in loop.')
  })

  it('shows every other passage, and a first one not led by the title, as it is', () => {
    const later = { position: 3, content: '#web\n\nquoted later on' }
    const untitled = { position: 0, content: 'Plans for the launch.' }

    expect(withoutTitle(later, '#web')).toBe(later.content)
    expect(withoutTitle(untitled, '#web')).toBe(untitled.content)
    expect(withoutTitle(untitled, '')).toBe(untitled.content)
  })
})

describe('presentKnowledgeSource for unknown sources', () => {
  const t = i18next.getFixedT('en-US', 'knowledge')

  it('reads a source this build does not know as its own name, with a plain icon', () => {
    expect(presentKnowledgeSource('github', t)).toEqual({
      label: 'github',
      icon: LuFileText,
      className: '',
    })
    // Not mistaken for one by a name every object answers to.
    expect(presentKnowledgeSource('toString', t).label).toBe('toString')
  })
})

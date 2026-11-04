// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// User interface
import { LuFileText, LuLightbulb } from 'react-icons/lu'

// Misc
import { KNOWLEDGE_SOURCES, presentKnowledgeSource, withoutTitle } from './knowledgePresentation'

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

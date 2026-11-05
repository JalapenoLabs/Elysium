// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { HTTPError } from 'ky'
import { describe, expect, it } from 'vitest'

// Misc
import {
  createLearningPageFormSchema,
  createPageFormSchema,
  describePublishRefusal,
  pageAsDiffText,
} from './pageForm'

const t = i18next.getFixedT('en-US', 'knowledge')

function refusal(data: unknown) {
  return Object.assign(Object.create(HTTPError.prototype), { data })
}

describe('createPageFormSchema', () => {
  it('collapses a title as the API does, and leaves the text alone', () => {
    const parsed = createPageFormSchema(t).parse({ title: '  Run  book ', body: '\n  Text\n\n' })

    expect(parsed).toEqual({ title: 'Run book', body: '\n  Text\n\n' })
  })

  it('needs a title, and may hold no text', () => {
    expect(createPageFormSchema(t).safeParse({ title: ' ', body: '' }).success).toBe(false)
    expect(createPageFormSchema(t).safeParse({ title: 'Holds others', body: '' }).success).toBe(true)
  })
})

describe('createLearningPageFormSchema', () => {
  const learning = { title: 'Lint first', body: 'Run it.', evidence: 'WAY-123.', category: 'convention' }

  it('needs its text and its evidence', () => {
    expect(createLearningPageFormSchema(t).safeParse(learning).success).toBe(true)
    expect(createLearningPageFormSchema(t).safeParse({ ...learning, body: ' ' }).success).toBe(false)
    expect(createLearningPageFormSchema(t).safeParse({ ...learning, evidence: '' }).success).toBe(false)
  })

  it('keeps its text as it is', () => {
    expect(createLearningPageFormSchema(t).parse({ ...learning, body: ' Run it.' }).body).toBe(' Run it.')
  })
})

describe('describePublishRefusal', () => {
  it('uses the app\'s own words for a refusal it knows', () => {
    expect(describePublishRefusal(refusal({ code: 'page_fixed', message: 'raw' }), t))
      .toBe('This page keeps its name.')
  })

  it('passes on the API\'s words for any other, such as a section\'s bound', () => {
    expect(describePublishRefusal(refusal({ message: 'at most 12,000 characters' }), t))
      .toBe('at most 12,000 characters')
  })
})

describe('pageAsDiffText', () => {
  it('leads with the title, and ends with a learning\'s evidence', () => {
    expect(pageAsDiffText({ title: 'Lint', body: 'Run it.', evidence: 'WAY-123.' }))
      .toBe('# Lint\n\nRun it.\n\nWAY-123.')
    expect(pageAsDiffText({ title: 'Lint', body: 'Run it.', evidence: null })).toBe('# Lint\n\nRun it.')
  })
})

// Copyright © 2026 Jalapeno Labs

import type { WorkBatch } from '../../api/routes/projectManagerRoutes'

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { DEFAULT_LOCALE } from '../../i18n'
import { makeWorkBatch } from '../../testFixtures'
import { createWorkBatchEditSchema, toWorkBatchEdits, toWorkBatchEditValues } from './workBatchEditSchema'

const NOW = Date.parse('2026-11-06T12:00:00Z')

// testSetup initializes the app's i18next instance with the en-US translations.
const t = i18next.getFixedT(DEFAULT_LOCALE, 'workQueue')
const codingT = i18next.getFixedT(DEFAULT_LOCALE, 'coding')
const schema = createWorkBatchEditSchema(t, codingT, NOW, { priority: 'normal', scheduledFor: null })

function proposed(overrides: Partial<WorkBatch> = {}) {
  return makeWorkBatch({ id: 'batch-1', ...overrides })
}

function firstIssue(values: unknown) {
  return schema.safeParse(values).error?.issues[0]?.message
}

describe('createWorkBatchEditSchema', () => {
  it('passes a batch as its Project Manager proposed it', () => {
    expect(schema.safeParse(toWorkBatchEditValues(proposed())).success).toBe(true)
  })

  it('needs a title and a goal, each within the API limits', () => {
    const values = toWorkBatchEditValues(proposed())
    expect(firstIssue({ ...values, title: '   ' })).toBe(t('edit.errors.titleRequired'))
    expect(firstIssue({ ...values, title: 'x'.repeat(201) })).toBe(t('edit.errors.titleTooLong', { max: 200 }))
    expect(firstIssue({ ...values, goal: '' })).toBe(t('edit.errors.goalRequired'))
  })

  it('holds repositories to a session\'s rules, each named once', () => {
    const values = toWorkBatchEditValues(proposed())
    expect(firstIssue({ ...values, repositories: [{ url: 'not a url', baseBranch: '' }]}))
      .toBe(codingT('create.errors.repositoryUrlInvalid'))
    const first = 'https://github.com/JalapenoLabs/Elysium.git'
    const second = 'https://github.com/jalapenolabs/elysium'
    const twice = [{ url: first, baseBranch: '' }, { url: second, baseBranch: 'main' }]
    expect(firstIssue({ ...values, repositories: twice }))
      .toBe(codingT('create.repositories.errors.duplicate', { first, second }))
  })

  it('needs a scheduled batch to be due ahead of now and within a year', () => {
    const values = { ...toWorkBatchEditValues(proposed()), priority: 'scheduled' }
    expect(firstIssue({ ...values, scheduledFor: null })).toBe(t('edit.errors.scheduleRequired'))
    expect(firstIssue({ ...values, scheduledFor: '2026-11-06T11:00:00Z' })).toBe(t('edit.errors.schedulePast'))
    expect(firstIssue({ ...values, scheduledFor: '2028-01-01T00:00:00Z' })).toBe(t('edit.errors.scheduleTooFar'))
    expect(schema.safeParse({ ...values, scheduledFor: '2026-11-07T09:00:00Z' }).success).toBe(true)
  })
})

describe('a proposal whose time lapsed while it waited', () => {
  const lapsed = proposed({ priority: 'scheduled', scheduledFor: '2026-11-05T09:00:00Z' })
  const lapsedSchema = createWorkBatchEditSchema(t, codingT, NOW, lapsed)

  it('is approved as proposed, sending no change', () => {
    const output = lapsedSchema.parse(toWorkBatchEditValues(lapsed))
    expect(toWorkBatchEdits(lapsed, output)).toEqual({})
  })

  it('still needs a time picked here to be ahead of now', () => {
    const values = { ...toWorkBatchEditValues(lapsed), scheduledFor: '2026-11-06T11:00:00Z' }
    expect(lapsedSchema.safeParse(values).error?.issues[0]?.message).toBe(t('edit.errors.schedulePast'))
  })
})

describe('toWorkBatchEdits', () => {
  it('sends nothing for a batch approved as proposed', () => {
    const batch = proposed()
    const output = schema.parse(toWorkBatchEditValues(batch))
    expect(toWorkBatchEdits(batch, output)).toEqual({})
  })

  it('sends only what changed, tidied, with a priority and its time together', () => {
    const batch = proposed()
    const output = schema.parse({
      ...toWorkBatchEditValues(batch),
      title: '  Annual pricing page  ',
      repositories: [{ url: 'https://github.com/JalapenoLabs/Website.git', baseBranch: ' develop ' }],
      priority: 'scheduled',
      scheduledFor: '2026-11-07T09:00:00Z',
    })
    expect(toWorkBatchEdits(batch, output)).toEqual({
      title: 'Annual pricing page',
      repositories: [{ url: 'https://github.com/JalapenoLabs/Website.git', baseBranch: 'develop' }],
      priority: 'scheduled',
      scheduledFor: '2026-11-07T09:00:00Z',
    })
  })

  it('reads the same instant in another spelling as unchanged', () => {
    const batch = proposed({ priority: 'scheduled', scheduledFor: '2026-11-07T09:00:00Z' })
    const output = schema.parse({
      ...toWorkBatchEditValues(batch),
      scheduledFor: '2026-11-07T09:00:00.000Z',
    })
    expect(toWorkBatchEdits(batch, output)).toEqual({})
  })
})

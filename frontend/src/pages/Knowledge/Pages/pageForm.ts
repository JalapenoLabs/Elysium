// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../../api/errors'
import { PAGE_BODY_MAX_CHARACTERS, PAGE_TITLE_MAX_CHARACTERS } from '../../../api/routes/pageRoutes'
import { TEXT_MAX_CHARACTERS } from '../Learnings/learningPresentation'

// A page's title and text as the API takes them, with the reasons Publish waits. A company doc
// section's tighter bound is the API's alone to say: only it knows which pages are sections.
export function createPageFormSchema(t: TFunction<'knowledge'>) {
  return z.object({
    title: z
      .string()
      .transform((title) => title.split(/\s+/u).filter(Boolean).join(' '))
      .pipe(z
        .string()
        .min(1, { error: t('pages.editor.titleRequired') })
        .max(PAGE_TITLE_MAX_CHARACTERS, { error: t('pages.editor.titleTooLong') })),
    body: z.string().max(PAGE_BODY_MAX_CHARACTERS, { error: t('pages.editor.bodyTooLong') }),
  })
}

// A learning holds more, and its body cannot be empty.
export function createLearningPageFormSchema(t: TFunction<'knowledge'>) {
  const text = (requiredMessage: string) => z
    .string()
    .max(TEXT_MAX_CHARACTERS, { error: t('pages.editor.learningTextTooLong') })
    .refine((value) => value.trim(), { error: requiredMessage })

  return createPageFormSchema(t).extend({
    body: text(t('pages.editor.learningBodyRequired')),
    evidence: text(t('pages.editor.evidenceRequired')),
    category: z.string().min(1),
  })
}

const refusalKeys = {
  page_deleted: 'pages.editor.refused.deleted',
  page_fixed: 'pages.editor.refused.fixed',
  page_parent_gone: 'pages.editor.refused.parentGone',
  page_title_taken: 'pages.editor.refused.titleTaken',
} as const

// What to tell a person whose publish was refused: the web app's own words for a refusal it knows,
// and otherwise the API's, which name a bound such as a company doc section's.
export function describePublishRefusal(error: unknown, t: TFunction<'knowledge'>) {
  const code = getApiErrorCode(error)
  if (code && code in refusalKeys) {
    return t(refusalKeys[code as keyof typeof refusalKeys])
  }
  return getApiErrorMessage(error)
}

// A version as the difference shows it: its title leads, so a new title shows as a change like any
// other, and a learning's evidence follows.
export function pageAsDiffText(version: { title: string, body: string, evidence?: string | null }) {
  const parts = [ `# ${version.title}`, version.body ]
  if (version.evidence) {
    parts.push(version.evidence)
  }
  return parts.join('\n\n')
}

// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { CalendarDate } from '@internationalized/date'
import { z } from 'zod'

// Misc
import { ACTION_ITEM_PRIORITIES } from '../../api/routes/actionItemRoutes'

// Mirrors the API's limits so most mistakes are caught before a round trip.
export const TITLE_MAX_CHARACTERS = 500
export const NOTES_MAX_CHARACTERS = 20_000

// Built per render with `t` so validation messages are already translated.
export function createActionItemFormSchema(t: TFunction<'actionItems'>) {
  return z.object({
    title: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.titleRequired') })
      .max(TITLE_MAX_CHARACTERS, { error: t('form.errors.titleTooLong') }),
    notes: z
      .string()
      .max(NOTES_MAX_CHARACTERS, { error: t('form.errors.notesTooLong') }),
    priority: z.enum(ACTION_ITEM_PRIORITIES),
    // A day in the viewer's zone; due until it ends.
    dueDate: z.instanceof(CalendarDate).nullable(),
    projectIds: z.array(z.string()),
    initiativeIds: z.array(z.string()),
  })
}

export type ActionItemFormInput = z.input<ReturnType<typeof createActionItemFormSchema>>
export type ActionItemFormValues = z.output<ReturnType<typeof createActionItemFormSchema>>

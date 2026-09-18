// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { JiraCredential } from '../../../api/routes/jiraRoutes'

// Utility
import { z } from 'zod'

// Misc
import { JIRA_SITE_URL_PATTERN, keepsStoredJiraToken, normalizeJiraSiteUrl } from './jiraPresentation'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const NAME_MAX_CHARACTERS = 120
const TOKEN_MAX_CHARACTERS = 255

// The connection half of a Jira credential: who Elysium signs in as, and where. The
// projects and boards it may read are picked from what Jira reports, so they carry no
// validation of their own and are held beside the form rather than in it.
//
// Built per render with `t` so validation messages are already translated. `stored` is the
// credential being edited, or null when adding one.
export function createJiraFormSchema(t: TFunction<'jira'>, stored: JiraCredential | null) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    siteUrl: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.siteUrlRequired') })
      .transform(normalizeJiraSiteUrl)
      .refine((value) => JIRA_SITE_URL_PATTERN.test(value), { error: t('form.errors.siteUrlShape') }),
    accountEmail: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.emailRequired') })
      .pipe(z.email({ error: t('form.errors.emailShape') })),
    token: z
      .string()
      .trim()
      .max(TOKEN_MAX_CHARACTERS, { error: t('form.errors.tokenTooLong') }),
  }).superRefine((values, context) => {
    if (values.token || keepsStoredJiraToken(stored, values.siteUrl, values.accountEmail)) {
      return
    }

    context.addIssue({
      code: 'custom',
      path: [ 'token' ],
      message: t('form.errors.tokenRequired'),
    })
  })
}

export type JiraFormInput = z.input<ReturnType<typeof createJiraFormSchema>>
export type JiraFormValues = z.output<ReturnType<typeof createJiraFormSchema>>

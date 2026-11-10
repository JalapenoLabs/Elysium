// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// The longest a meeting's title may be, as `MAX_TITLE_CHARACTERS` in
// api/src/routes/v1/meeting_uploads/create_upload.rs.
export const MEETING_TITLE_MAX_CHARACTERS = 2048

type SchemaOptions = {
  // A new meeting names its title and when it started; a capture takes its meeting's.
  isNewMeeting: boolean
  // A recording needs somewhere to be kept.
  hasRecording: boolean
}

// What an upload's form holds besides its files, which a form cannot keep as values: built with
// `t` so its messages arrive translated, mirroring what `POST /api/v1/meeting-uploads` refuses.
export function createMeetingUploadFormSchema(
  t: TFunction<'meetings'>,
  options: SchemaOptions,
) {
  return z
    .object({
      title: z.string(),
      // The instant it started, with its offset, or null before one is picked.
      startedAt: z.string().nullable(),
      storageLocationId: z.string(),
    })
    .superRefine((values, context) => {
      if (options.isNewMeeting) {
        const title = values.title.trim()
        if (!title) {
          context.addIssue({ code: 'custom', path: [ 'title' ], message: t('upload.errors.title') })
        }
        else if (title.length > MEETING_TITLE_MAX_CHARACTERS) {
          context.addIssue({
            code: 'custom',
            path: [ 'title' ],
            message: t('upload.errors.titleTooLong', { limit: String(MEETING_TITLE_MAX_CHARACTERS) }),
          })
        }
        if (!values.startedAt) {
          context.addIssue({ code: 'custom', path: [ 'startedAt' ], message: t('upload.errors.startedAt') })
        }
      }
      if (options.hasRecording && !values.storageLocationId) {
        context.addIssue({
          code: 'custom',
          path: [ 'storageLocationId' ],
          message: t('upload.errors.storageLocation'),
        })
      }
    })
}

export type MeetingUploadFormValues = z.infer<ReturnType<typeof createMeetingUploadFormSchema>>

// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Misc
import { ALL_PROJECTS } from '../../../api/routes/projectRoutes'
import { BUNNY_STORAGE_REGIONS, STORAGE_PROVIDER_KINDS } from '../../../api/routes/storageRoutes'
import { BYTES_PER_GIGABYTE } from '../../../constants'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const NAME_MAX_CHARACTERS = 120
const PATH_PREFIX_MAX_CHARACTERS = 1024
const ACCESS_KEY_MAX_BYTES = 1024
const BUNNY_ZONE_PATTERN = /^[A-Za-z0-9-]{1,64}$/

export type StorageFormMode = 'create' | 'edit'

// Built per render with `t` so validation messages are already translated.
export function createStorageFormSchema(t: TFunction<'storage'>, mode: StorageFormMode) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    kind: z.enum(STORAGE_PROVIDER_KINDS),
    zone: z
      .string()
      .trim()
      .regex(BUNNY_ZONE_PATTERN, { error: t('form.errors.zoneInvalid') }),
    region: z.enum(BUNNY_STORAGE_REGIONS),
    // Surrounding slashes are dropped, as the API stores the prefix without them.
    pathPrefix: z
      .string()
      .trim()
      .transform((value) => value.replace(/^\/+|\/+$/g, ''))
      .refine(
        (value) => value.length <= PATH_PREFIX_MAX_CHARACTERS,
        { error: t('form.errors.pathPrefixTooLong') },
      )
      .refine(
        (value) => !value || value.split('/').every((segment) => segment
          && segment !== '.'
          && segment !== '..'
          && !/[\\\p{Cc}]/u.test(segment)),
        { error: t('form.errors.pathPrefixInvalid') },
      ),
    projects: z.union([ z.literal(ALL_PROJECTS), z.array(z.string()) ]),
    isUnlimited: z.boolean(),
    // NaN while the field is empty. Checked below, and only when a limit applies.
    storageLimitGigabytes: z.number().or(z.nan()),
    accessKey: z
      .string()
      .refine(
        (value) => new TextEncoder().encode(value).length <= ACCESS_KEY_MAX_BYTES,
        { error: t('form.errors.accessKeyTooLong') },
      )
      .refine(
        // Editing keeps the stored access key when this is left blank.
        (value) => mode === 'edit' || value.trim().length > 0,
        { error: t('form.errors.accessKeyRequired') },
      ),
  }).superRefine((values, context) => {
    if (values.isUnlimited) {
      return
    }

    const bytes = Math.round(values.storageLimitGigabytes * BYTES_PER_GIGABYTE)
    if (!Number.isFinite(bytes) || bytes < 1) {
      context.addIssue({
        code: 'custom',
        path: [ 'storageLimitGigabytes' ],
        message: t('form.errors.storageLimitInvalid'),
      })
      return
    }
    if (bytes > Number.MAX_SAFE_INTEGER) {
      context.addIssue({
        code: 'custom',
        path: [ 'storageLimitGigabytes' ],
        message: t('form.errors.storageLimitTooLarge'),
      })
    }
  })
}

export type StorageFormInput = z.input<ReturnType<typeof createStorageFormSchema>>
export type StorageFormValues = z.output<ReturnType<typeof createStorageFormSchema>>

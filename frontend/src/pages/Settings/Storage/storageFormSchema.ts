// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Misc
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
    storageLimitGigabytes: z
      .number({ error: t('form.errors.storageLimitInvalid') })
      .refine(
        (value) => Math.round(value * BYTES_PER_GIGABYTE) >= 1,
        { error: t('form.errors.storageLimitInvalid') },
      )
      .refine(
        (value) => Math.round(value * BYTES_PER_GIGABYTE) <= Number.MAX_SAFE_INTEGER,
        { error: t('form.errors.storageLimitTooLarge') },
      ),
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
  })
}

export type StorageFormInput = z.input<ReturnType<typeof createStorageFormSchema>>
export type StorageFormValues = z.output<ReturnType<typeof createStorageFormSchema>>

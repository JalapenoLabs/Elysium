// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { StorageProvider } from '../../../api/routes/storageRoutes'
import type { StorageOption } from './storagePresentation'

// Utility
import { z } from 'zod'

// Misc
import { ALL_PROJECTS } from '../../../api/routes/projectRoutes'
import { BUNNY_STORAGE_REGIONS } from '../../../api/routes/storageRoutes'
import { BYTES_PER_GIGABYTE } from '../../../constants'
import { STORAGE_OPTIONS } from './storagePresentation'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const NAME_MAX_CHARACTERS = 120
const PATH_PREFIX_MAX_CHARACTERS = 1024
const ACCESS_KEY_MAX_BYTES = 1024
const BUNNY_ZONE_PATTERN = /^[A-Za-z0-9-]{1,64}$/
const BUCKET_PATTERN = /^[a-z0-9][a-z0-9._-]{1,220}[a-z0-9]$/
// Region codes such as us-east-1, eu-central-2, or us-gov-west-1.
const AWS_REGION_PATTERN = /^[a-z]{2}(-[a-z]+)+-\d+$/
const ACCESS_KEY_ID_PATTERN = /^[A-Za-z0-9]{1,256}$/

// Built per render with `t` so validation messages are already translated. `stored` is
// the provider being edited, or null when adding a location.
export function createStorageFormSchema(t: TFunction<'storage'>, stored: StorageProvider | null) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    option: z.enum(STORAGE_OPTIONS),
    zone: z.string().trim(),
    bunnyRegion: z.enum(BUNNY_STORAGE_REGIONS),
    bucket: z.string().trim(),
    awsRegion: z.string().trim(),
    accessKeyId: z.string().trim(),
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
      ),
  }).superRefine((values, context) => {
    function fail(path: string, message: string) {
      context.addIssue({ code: 'custom', path: [ path ], message })
    }

    // Only the chosen option's fields count; the others keep what was typed, unchecked.
    if (values.option === 'bunny' && !BUNNY_ZONE_PATTERN.test(values.zone)) {
      fail('zone', t('form.errors.zoneInvalid'))
    }
    if (values.option !== 'bunny') {
      if (!BUCKET_PATTERN.test(values.bucket)) {
        fail('bucket', t('form.errors.bucketInvalid'))
      }
      if (!ACCESS_KEY_ID_PATTERN.test(values.accessKeyId)) {
        fail('accessKeyId', t('form.errors.accessKeyIdInvalid'))
      }
    }
    if (values.option === 'aws' && !AWS_REGION_PATTERN.test(values.awsRegion)) {
      fail('awsRegion', t('form.errors.awsRegionInvalid'))
    }

    // A stored secret opens one zone or one access key id, as the API enforces.
    const needsAccessKey = !stored || !keepsAccessKey(toStorageProvider(values), stored)
    if (needsAccessKey && !values.accessKey.trim()) {
      fail('accessKey', t('form.errors.accessKeyRequired'))
    }

    if (values.isUnlimited) {
      return
    }
    const bytes = Math.round(values.storageLimitGigabytes * BYTES_PER_GIGABYTE)
    if (!Number.isFinite(bytes) || bytes < 1) {
      fail('storageLimitGigabytes', t('form.errors.storageLimitInvalid'))
      return
    }
    if (bytes > Number.MAX_SAFE_INTEGER) {
      fail('storageLimitGigabytes', t('form.errors.storageLimitTooLarge'))
    }
  })
}

export type StorageFormInput = z.input<ReturnType<typeof createStorageFormSchema>>
export type StorageFormValues = z.output<ReturnType<typeof createStorageFormSchema>>

type ProviderFields = Pick<StorageFormInput, 'option' | 'zone' | 'bunnyRegion' | 'bucket' | 'awsRegion' | 'accessKeyId'>

// Each option's provider, built from the fields that option shows.
const providerByOption = {
  'bunny': (fields) => ({
    kind: 'bunny',
    zone: fields.zone.trim(),
    region: fields.bunnyRegion,
  }),
  'aws': (fields) => ({
    kind: 's3',
    service: 'aws',
    bucket: fields.bucket.trim(),
    region: fields.awsRegion.trim(),
    accessKeyId: fields.accessKeyId.trim(),
  }),
  'google-cloud': (fields) => ({
    kind: 's3',
    service: 'google-cloud',
    bucket: fields.bucket.trim(),
    region: null,
    accessKeyId: fields.accessKeyId.trim(),
  }),
} as const satisfies Record<StorageOption, (fields: ProviderFields) => StorageProvider>

export function toStorageProvider(fields: ProviderFields): StorageProvider {
  return providerByOption[fields.option](fields)
}

// Mirrors `StorageProvider::keeps_access_key_of` in the API: a Bunny password belongs to
// one zone, and an S3 secret to one access key id on one service.
export function keepsAccessKey(next: StorageProvider, stored: StorageProvider) {
  if (next.kind === 'bunny' && stored.kind === 'bunny') {
    return next.zone === stored.zone
  }
  if (next.kind === 's3' && stored.kind === 's3') {
    return next.service === stored.service && next.accessKeyId === stored.accessKeyId
  }
  return false
}

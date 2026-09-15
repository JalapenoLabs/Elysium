// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  BunnyStorageRegion,
  StorageLocation,
  StorageProvider,
} from '../../../api/routes/storageRoutes'

// What the Provider picker offers. The API has two kinds, Bunny and S3; S3 is offered
// once per service, since each service is set up in its own console.
export const STORAGE_OPTIONS = [ 'bunny', 'aws', 'google-cloud' ] as const
export type StorageOption = typeof STORAGE_OPTIONS[number]

export function getStorageOption(provider: StorageProvider): StorageOption {
  if (provider.kind === 'bunny') {
    return 'bunny'
  }
  return provider.service
}

export const storageOptionLabelKeys = {
  'bunny': 'providers.bunny',
  'aws': 'providers.aws',
  'google-cloud': 'providers.google-cloud',
} as const satisfies Record<StorageOption, ParseKeys<'storage'>>

// Each option's words for the secret and, for S3, the key id it pairs with.
export const accessKeyLabelKeys = {
  'bunny': { secret: 'form.secret.bunny', secretHint: 'form.secretHint.bunny', keyId: null, keyIdHint: null },
  'aws': {
    secret: 'form.secret.aws',
    secretHint: 'form.secretHint.aws',
    keyId: 'form.keyId.aws',
    keyIdHint: 'form.keyIdHint.aws',
  },
  'google-cloud': {
    secret: 'form.secret.google-cloud',
    secretHint: 'form.secretHint.google-cloud',
    keyId: 'form.keyId.google-cloud',
    keyIdHint: 'form.keyIdHint.google-cloud',
  },
} as const satisfies Record<StorageOption, {
  secret: ParseKeys<'storage'>
  secretHint: ParseKeys<'storage'>
  keyId: ParseKeys<'storage'> | null
  keyIdHint: ParseKeys<'storage'> | null
}>

// One setup step. The text is translated; URLs are not.
export type StorageSetupStep = {
  textKey: ParseKeys<'storage'>
  link?: string
}

// Where each option keeps the settings the form asks for, in the provider's own words.
export const storageSetupStepsByOption = {
  'bunny': [
    { textKey: 'setup.bunny.open', link: 'https://dash.bunny.net/storage' },
    { textKey: 'setup.bunny.zone' },
    { textKey: 'setup.bunny.access' },
    { textKey: 'setup.bunny.region' },
    { textKey: 'setup.bunny.password' },
  ],
  'aws': [
    { textKey: 'setup.aws.bucket', link: 'https://console.aws.amazon.com/s3/buckets' },
    { textKey: 'setup.aws.region' },
    { textKey: 'setup.aws.user', link: 'https://console.aws.amazon.com/iam/home#/users' },
    { textKey: 'setup.aws.policy' },
    { textKey: 'setup.aws.key' },
  ],
  'google-cloud': [
    { textKey: 'setup.google-cloud.bucket', link: 'https://console.cloud.google.com/storage/browser' },
    { textKey: 'setup.google-cloud.role' },
    {
      textKey: 'setup.google-cloud.interoperability',
      link: 'https://console.cloud.google.com/storage/settings;tab=interoperability',
    },
    { textKey: 'setup.google-cloud.key' },
  ],
} as const satisfies Record<StorageOption, readonly StorageSetupStep[]>

export const bunnyRegionLabelKeys = {
  'frankfurt': 'regions.frankfurt',
  'london': 'regions.london',
  'new-york': 'regions.new-york',
  'los-angeles': 'regions.los-angeles',
  'singapore': 'regions.singapore',
  'stockholm': 'regions.stockholm',
  'sao-paulo': 'regions.sao-paulo',
  'johannesburg': 'regions.johannesburg',
  'sydney': 'regions.sydney',
} as const satisfies Record<BunnyStorageRegion, ParseKeys<'storage'>>

// Largest first: a limit is shown in the largest decimal unit it reaches.
const STORAGE_UNITS = [
  { unit: 'petabyte', bytes: 1e15 },
  { unit: 'terabyte', bytes: 1e12 },
  { unit: 'gigabyte', bytes: 1e9 },
  { unit: 'megabyte', bytes: 1e6 },
  { unit: 'kilobyte', bytes: 1e3 },
] as const

// A byte count as the viewer's locale writes it, such as "50 GB" or "1.5 TB".
export function formatStorageBytes(bytes: number, locale: string) {
  const { unit, bytes: unitBytes } = STORAGE_UNITS.find((candidate) => bytes >= candidate.bytes)
    ?? { unit: 'byte', bytes: 1 }

  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit,
    unitDisplay: 'short',
    maximumFractionDigits: 2,
  }).format(bytes / unitBytes)
}

// Where in the provider the location's files go, such as "elysium-files/uploads".
export function getStoragePath(location: StorageLocation) {
  const root = location.provider.kind === 'bunny'
    ? location.provider.zone
    : location.provider.bucket
  return [ root, location.pathPrefix ]
    .filter(Boolean)
    .join('/')
}

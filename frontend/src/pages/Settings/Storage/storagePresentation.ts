// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { BunnyStorageRegion, StorageLocation, StorageProviderKind } from '../../../api/routes/storageRoutes'

// One setup step. The text is translated; URLs are not.
export type StorageSetupStep = {
  textKey: ParseKeys<'storage'>
  link?: string
}

// Where each provider keeps the settings the form asks for, in the provider's own words.
export const storageSetupStepsByKind = {
  bunny: [
    { textKey: 'setup.bunny.open', link: 'https://dash.bunny.net/storage' },
    { textKey: 'setup.bunny.zone' },
    { textKey: 'setup.bunny.access' },
    { textKey: 'setup.bunny.region' },
    { textKey: 'setup.bunny.password' },
  ],
} as const satisfies Record<StorageProviderKind, readonly StorageSetupStep[]>

export const storageProviderLabelKeys = {
  bunny: 'providers.bunny',
} as const satisfies Record<StorageProviderKind, ParseKeys<'storage'>>

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
  return [ location.provider.zone, location.pathPrefix ]
    .filter(Boolean)
    .join('/')
}

// Copyright © 2026 Jalapeno Labs

// Misc
import { numberFormat } from './intl'

// Numbers as people read them. A large one compresses to its unit with one decimal, rounded
// rather than cut off: 1,350,000 is "1.4M". `Intl`'s compact notation writes K, M, and B in
// uppercase, so an "m" is never read as meters or minutes. Money keeps two decimals.

// Largest first: a size is shown in the largest decimal unit it reaches, as drives and
// storage providers count them.
const BYTE_UNITS = [
  { unit: 'petabyte', bytes: 1e15 },
  { unit: 'terabyte', bytes: 1e12 },
  { unit: 'gigabyte', bytes: 1e9 },
  { unit: 'megabyte', bytes: 1e6 },
  { unit: 'kilobyte', bytes: 1e3 },
] as const

// "1.4M", "12.3", "999". Translations reach it as `{{count, compact}}` (`src/i18n.ts`).
export function formatNumber(value: number) {
  return numberFormat({ notation: 'compact', maximumFractionDigits: 1 }).format(value)
}

// A share from 0 to 1 as a percentage: 0.784 is "78.4%".
export function formatPercent(ratio: number) {
  return numberFormat({ style: 'percent', maximumFractionDigits: 1 }).format(ratio)
}

// A byte count in the largest unit it reaches: "50 GB", "1.5 TB", or "512 bytes" under a kilobyte,
// whose short form ("512 byte") reads as a typo.
export function formatBytes(bytes: number) {
  const largest = BYTE_UNITS.find((candidate) => bytes >= candidate.bytes)
  if (!largest) {
    return numberFormat({ style: 'unit', unit: 'byte', unitDisplay: 'long' }).format(bytes)
  }

  return numberFormat({ style: 'unit', unit: largest.unit, unitDisplay: 'short', maximumFractionDigits: 1 })
    .format(bytes / largest.bytes)
}

// An amount of money in its currency, always with two decimals: "$1,234.50".
export function formatMoney(amount: number, currency: string) {
  return numberFormat({ style: 'currency', currency, minimumFractionDigits: 2, maximumFractionDigits: 2 })
    .format(amount)
}

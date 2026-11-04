// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { formatBytes, formatMoney, formatNumber, formatPercent } from './numbers'

describe('formatNumber', () => {
  it('compresses large numbers to one decimal with uppercase units', () => {
    expect(formatNumber(1_350_000)).toBe('1.4M')
    expect(formatNumber(2_500_000_000)).toBe('2.5B')
    expect(formatNumber(1_450)).toBe('1.5K')
  })

  it('rounds rather than cuts off', () => {
    expect(formatNumber(1_299_000)).toBe('1.3M')
    expect(formatNumber(12.36)).toBe('12.4')
  })

  it('leaves small whole numbers as they are', () => {
    expect(formatNumber(0)).toBe('0')
    expect(formatNumber(999)).toBe('999')
  })
})

describe('the compact translation format', () => {
  it('compresses a count while still choosing its plural', () => {
    expect(i18next.t('common:table.results', { count: 1_350_000 })).toBe('1.4M results')
    expect(i18next.t('common:table.results', { count: 1 })).toBe('1 result')
  })
})

describe('formatPercent', () => {
  it('writes a share to one decimal', () => {
    expect(formatPercent(0.784)).toBe('78.4%')
    expect(formatPercent(0.8)).toBe('80%')
    expect(formatPercent(1)).toBe('100%')
  })
})

describe('formatBytes', () => {
  it('writes a size in the largest unit it reaches, to one decimal', () => {
    expect(formatBytes(50e9)).toBe('50 GB')
    expect(formatBytes(1.55e12)).toBe('1.6 TB')
    expect(formatBytes(2_048)).toBe('2 kB')
  })

  it('writes a size under a kilobyte in bytes', () => {
    expect(formatBytes(512)).toBe('512 bytes')
  })
})

describe('formatMoney', () => {
  it('always keeps two decimals', () => {
    expect(formatMoney(1_234.5, 'USD')).toBe('$1,234.50')
    expect(formatMoney(3, 'USD')).toBe('$3.00')
  })
})

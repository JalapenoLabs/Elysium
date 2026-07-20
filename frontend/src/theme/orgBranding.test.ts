// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { hexToHslTriple } from './orgBranding'

describe('hexToHslTriple', () => {
  it('converts primaries and grays to HeroUI-style triples', () => {
    expect(hexToHslTriple('#ff0000')).toBe('0 100% 50%')
    expect(hexToHslTriple('#00ff00')).toBe('120 100% 50%')
    expect(hexToHslTriple('#0000ff')).toBe('240 100% 50%')
    expect(hexToHslTriple('#ffffff')).toBe('0 0% 100%')
    expect(hexToHslTriple('#000000')).toBe('0 0% 0%')
  })

  it('handles hues past the blue wrap-around without going negative', () => {
    // Pink sits in the 300s; the negative-hue branch must wrap it.
    expect(hexToHslTriple('#ec4899')).toBe('330 81% 60%')
  })

  it('accepts case and surrounding whitespace', () => {
    expect(hexToHslTriple('  #EC4899  ')).toBe('330 81% 60%')
  })

  it('rejects anything that is not a six-digit hex color', () => {
    expect(hexToHslTriple('#fff')).toBeNull()
    expect(hexToHslTriple('ec4899')).toBeNull()
    expect(hexToHslTriple('#ec489g')).toBeNull()
    expect(hexToHslTriple('')).toBeNull()
  })
})

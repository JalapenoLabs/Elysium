// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { toKeysym } from './keysyms'

describe('toKeysym', () => {
  it('types Latin-1 characters as their own code point', () => {
    expect(toKeysym({ key: 'a', code: 'KeyA' })).toBe(0x61)
    expect(toKeysym({ key: 'A', code: 'KeyA' })).toBe(0x41)
    expect(toKeysym({ key: ' ', code: 'Space' })).toBe(0x20)
    expect(toKeysym({ key: '@', code: 'Digit2' })).toBe(0x40)
    expect(toKeysym({ key: 'é', code: 'KeyE' })).toBe(0xe9)
  })

  it('types every other character through the Unicode keysym range', () => {
    expect(toKeysym({ key: 'ł', code: 'KeyL' })).toBe(0x01000142)
    expect(toKeysym({ key: '€', code: 'KeyE' })).toBe(0x010020ac)
    // A character outside the Basic Multilingual Plane is one key, not two halves.
    expect(toKeysym({ key: '😀', code: '' })).toBe(0x0101f600)
  })

  it('keeps left and right modifiers apart', () => {
    expect(toKeysym({ key: 'Shift', code: 'ShiftLeft' })).toBe(0xffe1)
    expect(toKeysym({ key: 'Shift', code: 'ShiftRight' })).toBe(0xffe2)
    expect(toKeysym({ key: 'Control', code: 'ControlLeft' })).toBe(0xffe3)
    expect(toKeysym({ key: 'Alt', code: 'AltRight' })).toBe(0xffea)
  })

  it('names the keys that type no character', () => {
    expect(toKeysym({ key: 'Enter', code: 'Enter' })).toBe(0xff0d)
    expect(toKeysym({ key: 'Backspace', code: 'Backspace' })).toBe(0xff08)
    expect(toKeysym({ key: 'Tab', code: 'Tab' })).toBe(0xff09)
    expect(toKeysym({ key: 'ArrowDown', code: 'ArrowDown' })).toBe(0xff54)
    expect(toKeysym({ key: 'F12', code: 'F12' })).toBe(0xffc9)
  })

  it('refuses keys Neko cannot type', () => {
    expect(toKeysym({ key: 'Dead', code: 'Quote' })).toBeNull()
    expect(toKeysym({ key: 'Unidentified', code: '' })).toBeNull()
  })
})

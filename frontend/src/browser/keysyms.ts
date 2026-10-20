// Copyright © 2026 Jalapeno Labs

// Turns a key the viewer pressed into the X11 keysym Neko types on the remote desktop.
//
// `KeyboardEvent.key` already says what the key means on the viewer's own layout, so a
// printable character maps straight to its keysym: Latin-1 code points are their own keysym,
// and every other Unicode character is 0x01000000 plus its code point (X11's keysym
// encoding). Keys without a character come from the tables below: modifiers by
// `KeyboardEvent.code`, so left and right stay distinct, and the rest by `key`.

// Keysyms from X11's keysymdef.h.
const keysymByModifierCode: Record<string, number> = {
  ShiftLeft: 0xffe1,
  ShiftRight: 0xffe2,
  ControlLeft: 0xffe3,
  ControlRight: 0xffe4,
  CapsLock: 0xffe5,
  MetaLeft: 0xffeb,
  MetaRight: 0xffec,
  AltLeft: 0xffe9,
  AltRight: 0xffea,
}

const keysymByNamedKey: Record<string, number> = {
  Backspace: 0xff08,
  Tab: 0xff09,
  Enter: 0xff0d,
  Pause: 0xff13,
  ScrollLock: 0xff14,
  Escape: 0xff1b,
  Home: 0xff50,
  ArrowLeft: 0xff51,
  ArrowUp: 0xff52,
  ArrowRight: 0xff53,
  ArrowDown: 0xff54,
  PageUp: 0xff55,
  PageDown: 0xff56,
  End: 0xff57,
  PrintScreen: 0xff61,
  Insert: 0xff63,
  ContextMenu: 0xff67,
  NumLock: 0xff7f,
  AltGraph: 0xfe03,
  F1: 0xffbe,
  F2: 0xffbf,
  F3: 0xffc0,
  F4: 0xffc1,
  F5: 0xffc2,
  F6: 0xffc3,
  F7: 0xffc4,
  F8: 0xffc5,
  F9: 0xffc6,
  F10: 0xffc7,
  F11: 0xffc8,
  F12: 0xffc9,
  Delete: 0xffff,
}

// The highest code point X11 keysyms give their own value to (Latin-1).
const LATIN_1_LAST_CODE_POINT = 0xff
const UNICODE_KEYSYM_OFFSET = 0x01000000

// What `toKeysym` reads from a KeyboardEvent.
export type KeyIdentity = Pick<KeyboardEvent, 'key' | 'code'>

// The keysym for a key, or null for one Neko cannot type (a dead key, an unknown name).
export function toKeysym(event: KeyIdentity): number | null {
  const modifier = keysymByModifierCode[event.code]
  if (modifier !== undefined) {
    return modifier
  }

  const named = keysymByNamedKey[event.key]
  if (named !== undefined) {
    return named
  }

  // A printable key's `key` is the one character it types; anything longer is a name.
  const characters = [ ...event.key ]
  if (characters.length !== 1) {
    return null
  }

  const codePoint = characters[0].codePointAt(0)
  if (codePoint === undefined) {
    return null
  }
  if (codePoint <= LATIN_1_LAST_CODE_POINT) {
    return codePoint
  }
  return UNICODE_KEYSYM_OFFSET + codePoint
}

// Control and V, which paste what `clipboard/set` put on the remote clipboard.
export const PASTE_KEYSYMS = {
  control: keysymByModifierCode.ControlLeft,
  letterV: 'v'.codePointAt(0) ?? 0x76,
} as const

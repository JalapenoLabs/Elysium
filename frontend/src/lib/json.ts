// Copyright © 2026 Jalapeno Labs

import type { ParseError } from 'jsonc-parser'

// Lib
import { parse } from 'jsonc-parser'

// Parses JSONC (JSON with comments and trailing commas) safely.
//
// Returns null on malformed input, logging why, so callers never throw on bad
// data. Uses Microsoft's `jsonc-parser` so our JSON files (translations, config)
// can carry comments. This is the one JSON entry point; prefer it over
// `JSON.parse` for any file we author.
export function parseJsonSafe<Shape = unknown>(text: string): Shape | null {
  const errors: ParseError[] = []
  const value = parse(text, errors, { allowTrailingComma: true })

  if (errors.length) {
    console.debug('parseJsonSafe: malformed JSONC, returning null', { errors })
    return null
  }

  return value
}

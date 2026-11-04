// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { DEFAULT_LOCALE, resources } from '../i18n'

// Every string in a namespace, by its full key, such as `common:table.results_one`.
function stringsOf(namespace: string, tree: object, prefix = ''): Array<[ string, string ]> {
  const strings: Array<[ string, string ]> = []
  for (const [ key, value ] of Object.entries(tree)) {
    const path = prefix
      ? `${prefix}.${key}`
      : `${namespace}:${key}`
    if (typeof value === 'string') {
      strings.push([ path, value ])
    }
    else if (value && typeof value === 'object') {
      strings.push(...stringsOf(namespace, value, path))
    }
  }
  return strings
}

describe('translations', () => {
  it('compress every count through the shared number format', () => {
    const uncompressed: string[] = []
    for (const [ namespace, tree ] of Object.entries(resources[DEFAULT_LOCALE])) {
      for (const [ key, text ] of stringsOf(namespace, tree)) {
        // A day's ordinal counts the day of the month, which is never compressed.
        if (text.includes('{{count}}') && !key.includes('.ordinalDay_')) {
          uncompressed.push(key)
        }
      }
    }

    // Write `{{count, compact}}`, so 1,350,000 records read "1.4M records".
    expect(uncompressed).toEqual([])
  })
})

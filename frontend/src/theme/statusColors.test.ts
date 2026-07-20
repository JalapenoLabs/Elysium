// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// Misc
import { statusKinds, statusTokens } from './statusColors'
import { hexToHslTriple } from './orgBranding'

// The status tokens live twice on purpose: as CSS variables in index.css (what the
// app renders) and as hexes in statusColors.ts (for JS consumers). These tests are
// the sync contract between the two, so a change to one side fails loudly until the
// other follows.
// Vitest runs with the frontend package as its working directory.
const indexCss = readFileSync(resolve(process.cwd(), 'src/index.css'), 'utf8')
const tailwindConfig = readFileSync(resolve(process.cwd(), 'tailwind.config.js'), 'utf8')

function extractCssBlock(selector: string): string {
  const match = indexCss.match(new RegExp(`${selector}\\s*\\{([^}]*)\\}`))
  expect(match, `index.css must declare a "${selector}" block`).toBeTruthy()
  return match![1]
}

describe('statusTokens', () => {
  const rootBlock = extractCssBlock(':root')
  const darkBlock = extractCssBlock('\\.dark')

  for (const kind of statusKinds) {
    const token = statusTokens[kind]

    it(`${kind}: light CSS variable matches ${token.lightHex}`, () => {
      const declared = rootBlock.match(new RegExp(`${token.cssVar}:\\s*([^;]+);`))?.[1].trim()
      expect(declared).toBe(hexToHslTriple(token.lightHex))
    })

    it(`${kind}: dark CSS variable matches ${token.darkHex}`, () => {
      const declared = darkBlock.match(new RegExp(`${token.cssVar}:\\s*([^;]+);`))?.[1].trim()
      expect(declared).toBe(hexToHslTriple(token.darkHex))
    })

    it(`${kind}: Tailwind exposes the ${token.cssVar} variable as a color`, () => {
      expect(tailwindConfig).toContain(`hsl(var(${token.cssVar}) / <alpha-value>)`)
    })
  }
})

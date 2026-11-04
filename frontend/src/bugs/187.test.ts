// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Utility
import ts from 'typescript'

// Issue #187: a button waiting on a request shows it working (`~/.claude/docs/ux.md`, Buttons).
// HeroUI 3.2 draws nothing for `isPending`, so `src/theme/matter.css` hides a pending button's
// children and spins a ring in their place. It can only hide elements: text written straight
// into a Button would stay on screen under the ring. Every Button's text is wrapped in a span,
// as `~/.claude/docs/typescript.md` asks, and this keeps it that way.

const sources = import.meta.glob<string>(
  [ '../**/*.tsx', '!../**/*.test.tsx' ],
  { query: '?raw', import: 'default', eager: true },
)

// Text, a translation, or a string, written as a Button's own child rather than inside an element.
function isBareText(child: ts.JsxChild) {
  if (ts.isJsxText(child)) {
    return child.getText().trim().length > 0
  }
  if (!ts.isJsxExpression(child) || !child.expression) {
    return false
  }
  const expression = child.expression
  if (ts.isStringLiteral(expression) || ts.isTemplateExpression(expression)) {
    return true
  }
  if (ts.isNoSubstitutionTemplateLiteral(expression)) {
    return true
  }
  // `{t('actions.save')}`, the common way text reaches a button.
  return ts.isCallExpression(expression) && expression.expression.getText() === 't'
}

describe('issue #187', () => {
  it('wraps every Button\'s text in an element, so a pending button can hide it', () => {
    const bare: string[] = []

    for (const path of Object.keys(sources)) {
      const source = ts.createSourceFile(path, sources[path], ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
      function walk(node: ts.Node) {
        if (ts.isJsxElement(node) && node.openingElement.tagName.getText() === 'Button') {
          for (const child of node.children) {
            if (isBareText(child)) {
              const line = source.getLineAndCharacterOfPosition(child.getStart()).line + 1
              bare.push(`${path}:${line}`)
            }
          }
        }
        ts.forEachChild(node, walk)
      }
      walk(source)
    }

    expect(bare).toEqual([])
  })
})

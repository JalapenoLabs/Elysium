// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Utility
import ts from 'typescript'

// Issue #171: an icon-only button names itself in a tooltip (`~/.claude/docs/ux.md`, Brevity), and
// every dropdown item leads with its icon (Dropdowns). About twenty buttons had no tooltip and
// most menu items no icon, so both rules are checked in every source file, wherever a button or
// a menu is written.

// Every source file, read as text and parsed, so the structure is checked rather than a render.
const sources = import.meta.glob<string>(
  [ '../**/*.tsx', '!../**/*.test.tsx' ],
  { query: '?raw', import: 'default', eager: true },
)

type JsxNode = ts.JsxElement | ts.JsxSelfClosingElement

// A dropdown item's children that are not its icon.
const NOT_AN_ICON = new Set([
  'Label',
  'Description',
  'Kbd',
  'Dropdown.ItemIndicator',
  'Dropdown.SubmenuIndicator',
])

function openingOf(node: JsxNode) {
  return ts.isJsxElement(node)
    ? node.openingElement
    : node
}

function tagOf(node: JsxNode) {
  return openingOf(node).tagName.getText()
}

function attributeOf(node: JsxNode, name: string) {
  return openingOf(node).attributes.properties.find((attribute) => {
    return ts.isJsxAttribute(attribute) && attribute.name.getText() === name
  })
}

// A HeroUI icon-only button: a Button that says so, or a link styled as one through
// `buttonVariants({ isIconOnly: true })`.
function isIconOnlyButton(node: JsxNode) {
  const tag = tagOf(node)
  if (tag === 'Button' || tag === 'ToggleButton') {
    return Boolean(attributeOf(node, 'isIconOnly'))
  }
  if (tag === 'Link') {
    return /isIconOnly:\s*true/.test(attributeOf(node, 'className')?.getText() ?? '')
  }
  return false
}

// Walks every JSX element in a file with the JSX elements around it, nearest last.
function walkJsx(path: string, visit: (node: JsxNode, ancestors: string[], where: string) => void) {
  const source = ts.createSourceFile(path, sources[path], ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  function walk(node: ts.Node, ancestors: string[]) {
    let inner = ancestors
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const line = source.getLineAndCharacterOfPosition(node.getStart()).line + 1
      visit(node, ancestors, `${path}:${line}`)
      inner = [ ...ancestors, tagOf(node) ]
    }
    ts.forEachChild(node, (child) => walk(child, inner))
  }
  walk(source, [])
}

describe('issue #171', () => {
  it('names every icon-only button, and shows the name in a tooltip it triggers', () => {
    const unnamed: string[] = []
    const withoutTooltip: string[] = []
    const wrappedInTrigger: string[] = []

    for (const path of Object.keys(sources)) {
      walkJsx(path, (node, ancestors, where) => {
        if (!isIconOnlyButton(node)) {
          return
        }
        if (!attributeOf(node, 'aria-label')) {
          unnamed.push(where)
        }
        if (!ancestors.includes('Tooltip')) {
          withoutTooltip.push(where)
        }
        // `Tooltip.Trigger` is a focusable element of its own, so a button inside it is a second
        // tab stop nested in the first. The button is the tooltip's trigger itself; only a
        // disabled one, which fires no hover, needs a wrapper (`PageSaveButton`).
        if (ancestors.at(-1) === 'Tooltip.Trigger') {
          wrappedInTrigger.push(where)
        }
      })
    }

    expect(unnamed).toEqual([])
    expect(withoutTooltip).toEqual([])
    expect(wrappedInTrigger).toEqual([])
  })

  it('leads every dropdown item with an icon', () => {
    const withoutIcon: string[] = []

    for (const path of Object.keys(sources)) {
      walkJsx(path, (node, _ancestors, where) => {
        if (tagOf(node) !== 'Dropdown.Item' || !ts.isJsxElement(node)) {
          return
        }
        let hasIcon = false
        for (const child of node.children) {
          // Content built elsewhere, such as a page action's, is checked where it is built.
          if (ts.isJsxExpression(child)) {
            hasIcon = true
          }
          if ((ts.isJsxElement(child) || ts.isJsxSelfClosingElement(child)) && !NOT_AN_ICON.has(tagOf(child))) {
            hasIcon = true
          }
        }
        if (!hasIcon) {
          withoutIcon.push(where)
        }
      })
    }

    expect(withoutIcon).toEqual([])
  })
})

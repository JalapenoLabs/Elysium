// Copyright © 2026 Jalapeno Labs

import type { Root, RootContent } from 'mdast'

// Utility
import remarkGfm from 'remark-gfm'
import remarkParse from 'remark-parse'
import remarkStringify from 'remark-stringify'
import { unified } from 'unified'

// The list markers a list already uses, so a new item matches the items around it.
export type ListStyle = {
  bullet: '-' | '*' | '+'
  bulletOrdered: '.' | ')'
}

export const DEFAULT_LIST_STYLE: ListStyle = {
  bullet: '-',
  bulletOrdered: '.',
}

// The same parser and dialect `Markdown` reads with, so the editor holds the page people read.
const parser = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .freeze()

export function parseMarkdown(markdown: string) {
  return parser.parse(markdown)
}

// How a block the editor changed is spelled. A table's pipes are not lined up, since padding one
// row to its widest cell would change every other row's line.
export function stringifyMarkdown(root: Root, style: ListStyle = DEFAULT_LIST_STYLE) {
  const written = unified()
    .use(remarkGfm, { tablePipeAlign: false })
    .use(remarkStringify, {
      ...style,
      closeAtx: false,
      emphasis: '*',
      fence: '`',
      fences: true,
      incrementListMarker: true,
      listItemIndent: 'one',
      resourceLink: false,
      rule: '-',
      setext: false,
      strong: '*',
    })
    .stringify(root)
  return written.replace(/\n+$/u, '')
}

// One block as a document of its own, since the parser and the stringifier both work on documents.
export function rootOf(...blocks: RootContent[]): Root {
  return { type: 'root', children: blocks }
}

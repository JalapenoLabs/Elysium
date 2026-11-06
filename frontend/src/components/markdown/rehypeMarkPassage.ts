// Copyright © 2026 Jalapeno Labs

import type { Root, RootContent } from 'hast'

// A stretch of the Markdown being read, as offsets into its text.
export type MarkdownRange = {
  start: number
  end: number
}

// What a marked block carries, for `src/theme/markdown.css` to draw and a reader to scroll to.
export const MARKED_PASSAGE_CLASS = 'markdown-passage'

// What holds blocks of its own, each with its own source position, so marking one covers more than
// was asked for when the range ends inside it, as a passage often does in a long list.
const BLOCK_CONTAINERS = new Set([ 'ul', 'ol', 'blockquote' ])

function markBlocks(blocks: RootContent[], range: MarkdownRange) {
  for (const block of blocks) {
    // Text between blocks, and what a plugin added, such as footnotes' section, has no place in
    // the source to compare.
    if (block.type !== 'element' || !block.position) {
      continue
    }
    const start = block.position.start.offset ?? 0
    const end = block.position.end.offset ?? 0
    if (end <= range.start || start >= range.end) {
      continue
    }

    const isCoveredWhole = start >= range.start && end <= range.end
    if (BLOCK_CONTAINERS.has(block.tagName) && !isCoveredWhole) {
      markBlocks(block.children, range)
      continue
    }

    const className = block.properties.className
    block.properties.className = Array.isArray(className)
      ? [ ...className, MARKED_PASSAGE_CLASS ]
      : [ MARKED_PASSAGE_CLASS ]
  }
}

type Options = {
  range: MarkdownRange
}

// Marks the blocks that `range` of the source reaches. It works on the syntax tree rather than the
// page, since every rendered block still knows where in the Markdown it came from, whatever
// rendering did to its text: a table's pipes, a list's markers, and a link's address are in a
// passage and in no rendered string.
export function rehypeMarkPassage(options: Options) {
  return (tree: Root) => {
    markBlocks(tree.children, options.range)
  }
}

// Copyright © 2026 Jalapeno Labs

import type { MarkdownRange } from '../../../components/markdown/rehypeMarkPassage'

// Misc
import { KNOWLEDGE_PASSAGE_PARAM } from '../../../urls'

const SEPARATOR = '\n\n'

// The passage an address opens at, if it names one a passage could be.
export function readPassageParam(params: URLSearchParams) {
  const position = Number(params.get(KNOWLEDGE_PASSAGE_PARAM))
  if (!params.has(KNOWLEDGE_PASSAGE_PARAM) || !Number.isInteger(position) || position < 0) {
    return null
  }
  return position
}

function skipWhitespace(text: string, index: number) {
  let next = index
  while (/\s/.test(text.charAt(next))) {
    next += 1
  }
  return next
}

// Where the run of `paragraphs` starting at `start` ends, as far as `body` keeps to them.
function findRunEnd(body: string, start: number, paragraphs: string[]) {
  let end = start
  for (const paragraph of paragraphs) {
    const next = skipWhitespace(body, end)
    if (!body.startsWith(paragraph, next)) {
      break
    }
    end = next + paragraph.length
  }
  return end
}

// Where a passage's text lies in a page's Markdown, or null when it does not. The index trims each
// paragraph it packs into a passage and joins them with one blank line, so a passage is not always
// a substring of what it was cut from: the paragraphs are found in turn, any whitespace between
// them. Evidence can follow a learning's body in one passage, and is not Markdown of the page, so
// the run keeps what the body holds of it. A paragraph said twice is found by the run that goes on
// longest, which only the passage's own place has.
export function findPassageRange(body: string, passage: string): MarkdownRange | null {
  const paragraphs = passage.split(SEPARATOR)
  const first = paragraphs[0]
  if (!first) {
    return null
  }

  let found: MarkdownRange | null = null
  let start = body.indexOf(first)
  while (start !== -1) {
    const end = findRunEnd(body, start, paragraphs)
    if (!found || end - start > found.end - found.start) {
      found = { start, end }
    }
    start = body.indexOf(first, start + 1)
  }
  return found
}

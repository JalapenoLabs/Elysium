// Copyright © 2026 Jalapeno Labs

import type { Node as ProseMirrorNode, Schema } from '@tiptap/pm/model'
import type { ListItem, RootContent, TableRow } from 'mdast'
import type { ListStyle } from './markdownSyntax'

// Utility
import { diffArrays } from 'diff'

// Misc
import { mdastToNode } from './mdastToNode'
import { DEFAULT_LIST_STYLE, parseMarkdown, rootOf, stringifyMarkdown } from './markdownSyntax'
import { nodeToMdast, withMarksNested } from './nodeToMdast'

// A part of a page as it was read: the node the editor holds for it, and the exact text it came
// from. A page stays Markdown that agents read and diffs show, so the text it was written in is the
// truth, and the editor's nodes are a way of changing it. See docs/frontend.md, The Markdown editor.
export type SourceBlock = {
  node: ProseMirrorNode
  text: string
  // What stands between this block and the one before it, or the page's start.
  gap: string
  // A top-level list's items and a table's rows, each kept the same way.
  parts: SourceBlock[]
  // The page's end closes it, as with a code fence never closed, so nothing can follow its text.
  isOpenEnded: boolean
}

export type MarkdownSource = {
  text: string
  document: ProseMirrorNode
  blocks: SourceBlock[]
  // What follows the last block.
  ending: string
  // The page's link definitions, which give brackets in its text their meaning.
  definitions: string
}

// A block in its new place: the block it is, or replaces, and whether it is still that block.
type PlacedBlock = {
  node: ProseMirrorNode
  original: SourceBlock | null
  originalIndex: number | null
  isUnchanged: boolean
}

type BlockPosition = {
  position?: { start: { offset?: number }, end: { offset?: number } }
}

const NEEDLESS_ESCAPE_CANDIDATE = /\\(?=[!-/:-@[-`{-~])/gu
const LIST_MARKER = /^\s*(?:([-*+])|(\d+)([.)]))\s/u
const OTHER_BULLET = '*'
const OTHER_ORDERED_DELIMITER = ')'

function sliceOf(block: BlockPosition, markdown: string) {
  return markdown.slice(block.position?.start.offset, block.position?.end.offset)
}

function childrenOf(node: ProseMirrorNode) {
  const children: ProseMirrorNode[] = []
  node.forEach((child) => children.push(child))
  return children
}

// Whether the editor's node says all that the Markdown does. Compared as spelled, the same way, so
// that how the source happened to be written doesn't count against it.
function isHeldWhole(block: RootContent, node: ProseMirrorNode) {
  return stringifyMarkdown(rootOf(withMarksNested(block))) === stringifyMarkdown(rootOf(nodeToMdast(node)))
}

function readParts(block: RootContent, node: ProseMirrorNode, markdown: string) {
  if (block.type !== 'list' && block.type !== 'table') {
    return []
  }
  const children: (ListItem | TableRow)[] = block.children
  return children.map((child, index): SourceBlock => {
    const previousEnd = children[index - 1]?.position?.end.offset
    return {
      node: node.child(index),
      text: sliceOf(child, markdown),
      gap: previousEnd === undefined
        ? ''
        : markdown.slice(previousEnd, child.position?.start.offset),
      parts: [],
      isOpenEnded: false,
    }
  })
}

// The block's text can have something after it, unless it only ends with the page.
function isClosed(text: string) {
  return parseMarkdown(`${text}\n\nafter`).children.length === parseMarkdown(text).children.length + 1
}

function readBlock(block: RootContent, markdown: string, schema: Schema, gap: string): SourceBlock {
  const text = sliceOf(block, markdown)
  const node = mdastToNode(block, markdown, schema)
  if (node && isHeldWhole(block, node)) {
    return { node, text, gap, parts: readParts(block, node, markdown), isOpenEnded: false }
  }
  return {
    node: schema.nodes.markdownBlock.create(null, schema.text(text)),
    text,
    gap,
    parts: [],
    isOpenEnded: false,
  }
}

// Reads a page's Markdown for the editor, keeping where every block came from.
export function readMarkdown(markdown: string, schema: Schema): MarkdownSource {
  const root = parseMarkdown(markdown)
  let end = 0
  const definitions: string[] = []
  const blocks = root.children.map((block) => {
    const gap = markdown.slice(end, block.position?.start.offset)
    end = block.position?.end.offset ?? end
    if (block.type === 'definition') {
      definitions.push(sliceOf(block, markdown))
    }
    return readBlock(block, markdown, schema, gap)
  })

  const last = blocks.at(-1)
  if (last) {
    last.isOpenEnded = !isClosed(last.text)
  }

  return {
    text: markdown,
    document: schema.nodes.doc.create(null, blocks.length
      ? blocks.map((block) => block.node)
      : schema.nodes.paragraph.create()),
    blocks,
    ending: markdown.slice(end),
    definitions: definitions.join('\n'),
  }
}

// Lines the current nodes up with the blocks read, so that each one still there keeps its text. A
// node in place of one that went replaces it, as an edit of it, whichever order they came in.
function placeBlocks(originals: SourceBlock[], nodes: ProseMirrorNode[]) {
  const changes = diffArrays(
    originals.map((original) => original.node),
    nodes,
    { comparator: (first, second) => first.eq(second) },
  )

  const placed: PlacedBlock[] = []
  let originalIndex = 0
  let nodeIndex = 0
  let replaced: number[] = []
  for (const change of changes) {
    const count = change.count ?? change.value.length
    if (change.removed) {
      replaced = Array.from({ length: count }, (_value, offset) => originalIndex + offset)
      originalIndex += count
      continue
    }
    for (let offset = 0; offset < count; offset += 1) {
      const node = nodes[nodeIndex]
      nodeIndex += 1
      if (change.added) {
        const replacedIndex = replaced.shift() ?? null
        placed.push({
          node,
          original: replacedIndex === null
            ? null
            : originals[replacedIndex],
          originalIndex: replacedIndex,
          isUnchanged: false,
        })
        continue
      }
      placed.push({ node, original: originals[originalIndex], originalIndex, isUnchanged: true })
      originalIndex += 1
    }
    replaced = []
  }
  return placed
}

// Joins blocks with the space they had, where they are still neighbours, and `separator` otherwise.
function joinBlocks(
  placed: PlacedBlock[],
  separator: string,
  write: (block: PlacedBlock, index: number, previousText: string) => string,
) {
  let written = ''
  let previousText = ''
  placed.forEach((block, index) => {
    const previous = placed[index - 1]
    const hasNeighbour = block.originalIndex !== null
      && (previous?.originalIndex ?? -1) === block.originalIndex - 1
    if (hasNeighbour && block.original) {
      written += block.original.gap
    }
    else if (index) {
      written += separator
    }
    previousText = write(block, index, previousText)
    written += previousText
  })
  return written
}

// mdast-util-to-markdown escapes whatever could be syntax, such as the `_` of `snake_case`, the `~`
// of `~/.config`, and every `[`. Each escape goes if the text means the same without it.
function dropNeedlessEscapes(written: string, definitions: string) {
  const meaningOf = (text: string) => stringifyMarkdown(parseMarkdown(`${text}\n\n${definitions}`))
  const meaning = meaningOf(written)
  const escapes = [ ...written.matchAll(NEEDLESS_ESCAPE_CANDIDATE) ]

  let kept = written
  for (const escape of escapes.reverse()) {
    const candidate = kept.slice(0, escape.index) + kept.slice(escape.index + 1)
    if (meaningOf(candidate) === meaning) {
      kept = candidate
    }
  }
  return kept
}

function writeNewBlock(node: ProseMirrorNode, definitions: string, style = DEFAULT_LIST_STYLE) {
  const written = stringifyMarkdown(rootOf(nodeToMdast(node)), style)
  if (node.type.spec.code) {
    return written
  }
  return dropNeedlessEscapes(written, definitions)
}

function listStyleOf(list: SourceBlock): ListStyle {
  const marker = LIST_MARKER.exec(list.text)
  return {
    bullet: (marker?.[1] ?? DEFAULT_LIST_STYLE.bullet) as ListStyle['bullet'],
    bulletOrdered: (marker?.[3] ?? DEFAULT_LIST_STYLE.bulletOrdered) as ListStyle['bulletOrdered'],
  }
}

function numberOf(item: SourceBlock) {
  return Number(LIST_MARKER.exec(item.text)?.[2])
}

// A list whose text changed keeps the text of every item that did not. An ordered list's item keeps
// it only while its number is still right: a number that needs another digit moves the item's text.
function writeList(original: SourceBlock, list: ProseMirrorNode, definitions: string) {
  const style = listStyleOf(original)
  const isOrdered = list.type.name === 'orderedList'
  const start: number = list.attrs.start ?? 1
  const step = original.parts.length > 1 && numberOf(original.parts[0]) === numberOf(original.parts[1])
    ? 0
    : 1

  return joinBlocks(
    placeBlocks(original.parts, childrenOf(list)),
    list.attrs.spread
      ? '\n\n'
      : '\n',
    (item, index) => {
      const number = start + index * step
      const isKept = item.isUnchanged
        && item.original
        && (!isOrdered || numberOf(item.original) === number)
      if (isKept) {
        return item.original?.text ?? ''
      }
      return writeNewBlock(list.type.create({ ...list.attrs, start: number }, item.node), definitions, style)
    },
  )
}

function columnsOf(table: ProseMirrorNode) {
  return childrenOf(table.child(0)).map((cell) => cell.attrs.align)
}

function hasSameColumns(first: ProseMirrorNode, second: ProseMirrorNode) {
  return columnsOf(first).join() === columnsOf(second).join()
    && first.child(0).childCount === second.child(0).childCount
}

// A table whose columns stand keeps the text of every row that did not change, and its delimiter
// row, so a padded table loses no padding to one edited cell. Only a table can be written, so a new
// row is the first line of a table of one.
function writeTable(original: SourceBlock, table: ProseMirrorNode, definitions: string) {
  const [ header, ...body ] = placeBlocks(original.parts, childrenOf(table))
  const writeRow = (row: PlacedBlock) => {
    if (row.isUnchanged && row.original) {
      return row.original.text
    }
    return writeNewBlock(table.type.create(table.attrs, row.node), definitions).split('\n')[0]
  }

  const delimiter = original.text.split('\n')[1]
  const lines = [ writeRow(header), delimiter ]
  if (body.length) {
    lines.push(joinBlocks(body, '\n', writeRow))
  }
  return lines.join('\n')
}

// Two lists side by side stay two only with different markers, or Markdown reads them as one.
function styleBeside(previousText: string): ListStyle {
  const marker = LIST_MARKER.exec(previousText)
  return {
    bullet: marker?.[1] === DEFAULT_LIST_STYLE.bullet
      ? OTHER_BULLET
      : DEFAULT_LIST_STYLE.bullet,
    bulletOrdered: marker?.[3] === DEFAULT_LIST_STYLE.bulletOrdered
      ? OTHER_ORDERED_DELIMITER
      : DEFAULT_LIST_STYLE.bulletOrdered,
  }
}

function writeChangedBlock(block: PlacedBlock, definitions: string, previousText: string) {
  const { node, original } = block
  if (original?.node.type === node.type && original.parts.length) {
    if (node.type.name === 'table') {
      if (hasSameColumns(original.node, node)) {
        return writeTable(original, node, definitions)
      }
    }
    else {
      return writeList(original, node, definitions)
    }
  }
  return writeNewBlock(node, definitions, styleBeside(previousText))
}

// Markdown cannot say a blank paragraph, nor a block of Markdown with nothing in it.
function isBlank(node: ProseMirrorNode) {
  return !node.content.size && (node.type.name === 'paragraph' || node.type.name === 'markdownBlock')
}

// The Markdown for `document`, which `source` was read into. Only what changed since is written:
// every block the person did not touch keeps its exact text, so a page opened and left alone is the
// same bytes, and an edit's diff is the edit.
export function writeMarkdown(source: MarkdownSource, document: ProseMirrorNode) {
  if (document.eq(source.document)) {
    return source.text
  }

  const nodes = childrenOf(document).filter((node) => !isBlank(node))
  const placed = placeBlocks(source.blocks, nodes)
  const lastIndex = placed.length - 1
  const body = joinBlocks(placed, '\n\n', (block, index, previousText) => {
    const isSafeToKeep = !block.original?.isOpenEnded || index === lastIndex
    if (block.isUnchanged && block.original && isSafeToKeep) {
      return block.original.text
    }
    return writeChangedBlock(block, source.definitions, previousText)
  })
  return body + source.ending
}

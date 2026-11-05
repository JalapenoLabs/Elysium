// Copyright © 2026 Jalapeno Labs

import type { Mark, Node as ProseMirrorNode } from '@tiptap/pm/model'
import type { BlockContent, List, ListItem, Nodes, PhrasingContent, Table, TableRow } from 'mdast'

// How marks that span exactly the same text nest. A document's marks lie flat, so `**[a](b)**` and
// `[**a**](b)` are one thing to it, and this is how either comes back.
const MARK_NESTING = [ 'link', 'italic', 'bold', 'strike' ]
const WRAPPER_NESTING = [ 'link', 'emphasis', 'strong', 'delete' ]

// A stretch of a text block under one set of marks: text and line endings carry a `value`, and
// anything that stands on its own, such as an image, a `node`.
type InlinePiece = {
  marks: readonly Mark[]
  value?: string
  node?: PhrasingContent
}

type OpenMark = {
  mark: Mark
  children: PhrasingContent[]
}

// Markdown's emphasis cannot open or close beside a space (`**word **` is not bold), so the spaces
// at a selection's edge step outside these marks.
const ATTENTION_MARKS = new Set([ 'bold', 'italic', 'strike' ])

function childrenOf(node: ProseMirrorNode) {
  const children: ProseMirrorNode[] = []
  node.forEach((child) => children.push(child))
  return children
}

const inlineReaders: Record<string, (node: ProseMirrorNode) => Omit<InlinePiece, 'marks'>> = {
  text: (node) => ({ value: node.text ?? '' }),
  hardBreak: (node) => node.attrs.soft
    ? { value: '\n' }
    : { node: { type: 'break' }},
  image: (node) => ({
    node: { type: 'image', url: node.attrs.src, alt: node.attrs.alt, title: node.attrs.title },
  }),
  // `html` is written exactly as it stands, whichever construct it came from.
  markdownInline: (node) => ({ node: { type: 'html', value: node.attrs.source }}),
}

function isCode(piece: InlinePiece) {
  return piece.marks.some((mark) => mark.type.name === 'code')
}

function hasMark(piece: InlinePiece | undefined, mark: Mark) {
  return Boolean(piece?.marks.some((candidate) => candidate.eq(mark)))
}

function keepAcrossSpace(marks: readonly Mark[], neighbor: InlinePiece | undefined) {
  return marks.filter((mark) => !ATTENTION_MARKS.has(mark.type.name) || hasMark(neighbor, mark))
}

// Moves the spaces at the edges of emphasis out of it, unless the emphasis goes on past them.
function expelEdgeSpaces(pieces: InlinePiece[]) {
  return pieces.flatMap((piece, index) => {
    if (piece.value === undefined || isCode(piece)) {
      return [ piece ]
    }
    const [ , leading, core, trailing ] = /^(\s*)(.*?)(\s*)$/su.exec(piece.value) ?? []
    const expelled: InlinePiece[] = []
    if (leading) {
      expelled.push({ value: leading, marks: keepAcrossSpace(piece.marks, pieces[index - 1]) })
    }
    if (core) {
      expelled.push({ value: core, marks: piece.marks })
    }
    if (trailing) {
      expelled.push({ value: trailing, marks: keepAcrossSpace(piece.marks, pieces[index + 1]) })
    }
    return expelled
  })
}

const markWriters: Record<string, (mark: Mark, children: PhrasingContent[]) => PhrasingContent> = {
  bold: (_mark, children) => ({ type: 'strong', children }),
  italic: (_mark, children) => ({ type: 'emphasis', children }),
  strike: (_mark, children) => ({ type: 'delete', children }),
  link: (mark, children) => ({ type: 'link', url: mark.attrs.href, title: mark.attrs.title, children }),
}

// How many pieces from `start` on keep `mark`. The mark that lasts longest opens first, so that
// `*a **b***` nests as it was written rather than closing and opening again.
function spanOf(mark: Mark, pieces: InlinePiece[], start: number) {
  let end = start
  while (hasMark(pieces[end], mark)) {
    end += 1
  }
  return end - start
}

function appendLeaf(children: PhrasingContent[], piece: InlinePiece) {
  if (piece.value === undefined) {
    if (piece.node) {
      children.push(piece.node)
    }
    return
  }
  const type = isCode(piece)
    ? 'inlineCode'
    : 'text'
  const last = children.at(-1)
  if (last?.type === type) {
    last.value += piece.value
    return
  }
  children.push({ type, value: piece.value })
}

// Marks lie flat on a text block's pieces and Markdown nests them, so each piece keeps open what it
// still has and opens the rest.
function nestMarks(pieces: InlinePiece[]) {
  const root: PhrasingContent[] = []
  const open: OpenMark[] = []

  pieces.forEach((piece, index) => {
    const marks = piece.marks.filter((mark) => mark.type.name !== 'code')
    let depth = 0
    while (depth < open.length && marks.some((mark) => mark.eq(open[depth].mark))) {
      depth += 1
    }
    open.length = depth

    const unopened = marks
      .filter((mark) => !open.some((entry) => entry.mark.eq(mark)))
      .sort((first, second) => spanOf(second, pieces, index) - spanOf(first, pieces, index)
        || MARK_NESTING.indexOf(first.type.name) - MARK_NESTING.indexOf(second.type.name))
    for (const mark of unopened) {
      const children: PhrasingContent[] = []
      const parent = open.at(-1)?.children ?? root
      parent.push(markWriters[mark.type.name](mark, children))
      open.push({ mark, children })
    }

    appendLeaf(open.at(-1)?.children ?? root, piece)
  })

  return root
}

// Spaces mean nothing at a line's edges, and Markdown would have to encode them to keep them.
function trimLineEdges(children: PhrasingContent[]) {
  return children.flatMap((child, index): PhrasingContent[] => {
    if (child.type !== 'text') {
      return [ child ]
    }
    let value = child.value.replace(/[ \t]*\n[ \t]*/gu, '\n')
    const previous = children[index - 1]
    const next = children[index + 1]
    if (!previous || previous.type === 'break') {
      value = value.trimStart()
    }
    if (!next || next.type === 'break') {
      value = value.trimEnd()
    }
    return value
      ? [{ ...child, value }]
      : []
  })
}

function writeInline(node: ProseMirrorNode) {
  const pieces = childrenOf(node).map((child) => ({
    ...inlineReaders[child.type.name](child),
    marks: child.marks,
  }))
  return trimLineEdges(nestMarks(expelEdgeSpaces(pieces)))
}

function writeListItem(item: ProseMirrorNode): ListItem {
  return {
    type: 'listItem',
    spread: item.attrs.spread,
    checked: item.type.name === 'taskItem'
      ? item.attrs.checked
      : null,
    children: childrenOf(item).map(nodeToMdast),
  }
}

function writeList(node: ProseMirrorNode): List {
  return {
    type: 'list',
    ordered: node.type.name === 'orderedList',
    start: node.attrs.start ?? null,
    spread: node.attrs.spread,
    children: childrenOf(node).map(writeListItem),
  }
}

// A cell is one line in Markdown, so a line break typed into one becomes a space.
function writeCell(cell: ProseMirrorNode): PhrasingContent[] {
  const children = cell.firstChild
    ? writeInline(cell.firstChild)
    : []
  return children.map((child) => child.type === 'break'
    ? { type: 'text', value: ' ' }
    : child)
}

function writeTable(node: ProseMirrorNode): Table {
  const rows = childrenOf(node)
  const headerCells = rows.length
    ? childrenOf(rows[0])
    : []
  return {
    type: 'table',
    align: headerCells.map((cell) => cell.attrs.align),
    children: rows.map((row): TableRow => ({
      type: 'tableRow',
      children: childrenOf(row).map((cell) => ({ type: 'tableCell', children: writeCell(cell) })),
    })),
  }
}

const blockWriters: Record<string, (node: ProseMirrorNode) => BlockContent> = {
  paragraph: (node) => ({ type: 'paragraph', children: writeInline(node) }),
  heading: (node) => ({ type: 'heading', depth: node.attrs.level, children: writeInline(node) }),
  blockquote: (node) => ({ type: 'blockquote', children: childrenOf(node).map(nodeToMdast) }),
  bulletList: writeList,
  orderedList: writeList,
  taskList: writeList,
  table: writeTable,
  horizontalRule: () => ({ type: 'thematicBreak' }),
  codeBlock: (node) => ({
    type: 'code',
    lang: node.attrs.language,
    meta: node.attrs.meta,
    value: node.textContent,
  }),
  // `html` is written exactly as it stands, whichever construct it came from.
  markdownBlock: (node) => ({ type: 'html', value: node.textContent }),
}

// `node` with marks that span the same text nested as `nestMarks` nests them, since their order
// says nothing, so a tree read from Markdown can be compared with one written from the document.
export function withMarksNested<Node extends Nodes>(node: Node): Node
export function withMarksNested(node: Nodes): Nodes {
  if (!('children' in node)) {
    return node
  }
  const children = node.children.map((child) => withMarksNested(child))
  const [ only ] = children
  const isOutOfOrder = children.length === 1
    && only
    && 'children' in only
    && WRAPPER_NESTING.includes(node.type)
    && WRAPPER_NESTING.includes(only.type)
    && WRAPPER_NESTING.indexOf(only.type) < WRAPPER_NESTING.indexOf(node.type)
  if (isOutOfOrder) {
    return withMarksNested({ ...only, children: [{ ...node, children: only.children }]} as Nodes)
  }
  return { ...node, children } as Nodes
}

// A block of the editor's document as a Markdown tree, which `stringifyMarkdown` spells.
export function nodeToMdast(node: ProseMirrorNode) {
  return blockWriters[node.type.name](node)
}

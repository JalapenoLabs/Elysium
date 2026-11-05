// Copyright © 2026 Jalapeno Labs

import type { JSONContent } from '@tiptap/core'
import type { Schema } from '@tiptap/pm/model'
import type { List, ListItem, PhrasingContent, RootContent, Table } from 'mdast'

// Core
import { Node as ProseMirrorNode } from '@tiptap/pm/model'

type EditorMark = NonNullable<JSONContent['marks']>[number]

// A paragraph's single line ending stays, as a soft break, rather than becoming a space: a
// hard-wrapped paragraph is then edited line by line, and a change touches one line of its source.
function readText(value: string, marks: EditorMark[]) {
  const content: JSONContent[] = []
  value.split('\n').forEach((line, index) => {
    if (index) {
      content.push({ type: 'hardBreak', attrs: { soft: true }, marks })
    }
    if (line) {
      content.push({ type: 'text', text: line, marks })
    }
  })
  return content
}

function readInline(nodes: PhrasingContent[], marks: EditorMark[], markdown: string): JSONContent[] {
  return nodes.flatMap((node): JSONContent[] => {
    switch (node.type) {
      case 'text':
        return readText(node.value, marks)
      case 'strong':
        return readInline(node.children, [ ...marks, { type: 'bold' }], markdown)
      case 'emphasis':
        return readInline(node.children, [ ...marks, { type: 'italic' }], markdown)
      case 'delete':
        return readInline(node.children, [ ...marks, { type: 'strike' }], markdown)
      case 'link': {
        const link = { type: 'link', attrs: { href: node.url, title: node.title ?? null }}
        return readInline(node.children, [ ...marks, link ], markdown)
      }
      case 'inlineCode':
        return [{ type: 'text', text: node.value, marks: [ ...marks, { type: 'code' }]}]
      case 'break':
        return [{ type: 'hardBreak', marks }]
      case 'image':
        return [{
          type: 'image',
          attrs: { src: node.url, alt: node.alt ?? null, title: node.title ?? null },
          marks,
        }]
      // These read as text wherever they stand, so each stays the text it was written as: raw
      // HTML, which `Markdown` shows rather than runs, and a reference to a definition elsewhere.
      default: {
        const start = node.position?.start.offset
        const end = node.position?.end.offset
        return [{ type: 'markdownInline', attrs: { source: markdown.slice(start, end) }, marks }]
      }
    }
  })
}

function readListItem(item: ListItem, type: string, markdown: string): JSONContent {
  const content = item.children.flatMap((child) => readBlock(child, markdown) ?? [])
  return {
    type,
    attrs: type === 'taskItem'
      ? { spread: item.spread, checked: item.checked }
      : { spread: item.spread },
    content: content.length
      ? content
      : [{ type: 'paragraph' }],
  }
}

function readList(list: List, markdown: string): JSONContent {
  // A task list is all tasks. One mixing tasks and plain items has no node, so it reads as a plain
  // list, which fails `readMarkdown`'s check and stays Markdown.
  const isTaskList = list.children.every((item) => typeof item.checked === 'boolean')
  if (isTaskList) {
    return {
      type: 'taskList',
      attrs: { spread: Boolean(list.spread) },
      content: list.children.map((item) => readListItem(item, 'taskItem', markdown)),
    }
  }
  return {
    type: list.ordered
      ? 'orderedList'
      : 'bulletList',
    attrs: list.ordered
      ? { spread: Boolean(list.spread), start: list.start ?? 1 }
      : { spread: Boolean(list.spread) },
    content: list.children.map((item) => readListItem(item, 'listItem', markdown)),
  }
}

function readTable(table: Table, markdown: string): JSONContent {
  return {
    type: 'table',
    content: table.children.map((row, rowIndex) => ({
      type: 'tableRow',
      content: row.children.map((cell, columnIndex) => ({
        // GitHub's tables always have one header row, the first.
        type: rowIndex
          ? 'tableCell'
          : 'tableHeader',
        attrs: { align: table.align?.[columnIndex] ?? null },
        content: [{ type: 'paragraph', content: readInline(cell.children, [], markdown) }],
      })),
    })),
  }
}

function readBlock(block: RootContent, markdown: string): JSONContent | null {
  switch (block.type) {
    case 'paragraph':
      return { type: 'paragraph', content: readInline(block.children, [], markdown) }
    case 'heading':
      return {
        type: 'heading',
        attrs: { level: block.depth },
        content: readInline(block.children, [], markdown),
      }
    case 'blockquote':
      return {
        type: 'blockquote',
        content: block.children.flatMap((child) => readBlock(child, markdown) ?? []),
      }
    case 'list':
      return readList(block, markdown)
    case 'table':
      return readTable(block, markdown)
    case 'thematicBreak':
      return { type: 'horizontalRule' }
    case 'code':
      return {
        type: 'codeBlock',
        attrs: { language: block.lang ?? null, meta: block.meta ?? null },
        content: block.value
          ? [{ type: 'text', text: block.value }]
          : [],
      }
    default:
      console.debug('The editor has no node for this Markdown block', { type: block.type })
      return null
  }
}

// `block` as a node of `schema`, or null when it has none or breaks the schema, such as a list item
// that starts with a code block. `markdown` is the text `block` was parsed from.
export function mdastToNode(block: RootContent, markdown: string, schema: Schema) {
  const content = readBlock(block, markdown)
  if (!content) {
    return null
  }
  try {
    const node = ProseMirrorNode.fromJSON(schema, content)
    node.check()
    return node
  }
  catch (error) {
    console.debug('The editor cannot hold this Markdown block', { type: block.type, error })
    return null
  }
}

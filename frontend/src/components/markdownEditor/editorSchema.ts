// Copyright © 2026 Jalapeno Labs

import type { AnyExtension } from '@tiptap/core'

// Core
import { Extension, Node } from '@tiptap/core'
import { Code } from '@tiptap/extension-code'
import { CodeBlockLowlight } from '@tiptap/extension-code-block-lowlight'
import { HardBreak } from '@tiptap/extension-hard-break'
import { Image } from '@tiptap/extension-image'
import { Link } from '@tiptap/extension-link'
import { TaskItem, TaskList } from '@tiptap/extension-list'
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table'
import { ReactNodeViewRenderer } from '@tiptap/react'
import { StarterKit } from '@tiptap/starter-kit'

// User interface
import { EditorImage } from './EditorImage'

// Utility
import { common, createLowlight } from 'lowlight'
import { defaultUrlTransform } from 'react-markdown'

// What Markdown says that the stock extensions have no attribute for, kept so that a block read
// from Markdown can be written back unchanged. None of it draws anything, apart from a column's
// alignment.
const MarkdownAttributes = Extension.create({
  name: 'markdownAttributes',
  addGlobalAttributes() {
    return [
      {
        // Whether a list or item has blank lines between its parts.
        types: [ 'bulletList', 'orderedList', 'taskList', 'listItem', 'taskItem' ],
        attributes: {
          spread: { default: false, rendered: false },
        },
      },
      {
        // The rest of a code fence's first line, after its language.
        types: [ 'codeBlock' ],
        attributes: {
          meta: { default: null, rendered: false },
        },
      },
      {
        types: [ 'tableCell', 'tableHeader' ],
        attributes: {
          align: {
            default: null,
            parseHTML: (element) => element.getAttribute('data-align'),
            renderHTML: (attributes) => attributes.align
              ? { 'data-align': attributes.align }
              : {},
          },
        },
      },
    ]
  },
})

// Markdown lets code sit inside bold or a link, which the stock mark refuses.
const MarkdownCode = Code.extend({ excludes: '' })

// Markdown has two line breaks: a hard one, and a soft one a paragraph keeps from its source. See
// `readText` in mdastToNode.ts.
const MarkdownBreak = HardBreak.extend({
  addAttributes() {
    return {
      soft: { default: false, rendered: false },
    }
  },
})

// Inline, as Markdown's is, so it can sit in a line or a link, and drawn by the page's own rule. Its
// plain HTML, for the clipboard and for a view being taken down, never carries `src`: an element
// with one fetches the moment it is made, attached or not, which is just what that rule forbids.
const MarkdownImage = Image.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      src: {
        default: null,
        parseHTML: (element) => element.getAttribute('data-src') ?? element.getAttribute('src'),
        renderHTML: (attributes) => ({ 'data-src': attributes.src }),
      },
    }
  },
  parseHTML() {
    return [{ tag: 'img[data-src]' }, ...this.parent?.() ?? [] ]
  },
  addNodeView() {
    // eslint-disable-next-line new-cap -- Tiptap's own name for a plain function.
    return ReactNodeViewRenderer(EditorImage)
  },
})

// A cell holds one line of Markdown, so one paragraph, which also leaves Enter nothing to split.
const MarkdownTableCell = TableCell.extend({ content: 'paragraph' })
const MarkdownTableHeader = TableHeader.extend({ content: 'paragraph' })

// A block the editor has no node for, or cannot hold without losing something: it opens as the
// Markdown it is, and stays exactly that until someone edits it as text.
const MarkdownBlock = Node.create({
  name: 'markdownBlock',
  group: 'block',
  content: 'text*',
  marks: '',
  code: true,
  defining: true,
  parseHTML() {
    return [{ tag: 'pre[data-markdown-block]', preserveWhitespace: 'full' }]
  },
  renderHTML() {
    return [ 'pre', { 'data-markdown-block': '' }, [ 'code', 0 ]]
  },
})

// The same inside a line: raw HTML, or a reference to a definition elsewhere on the page. One token,
// removed whole, shown as it reads on the page.
const MarkdownInline = Node.create({
  name: 'markdownInline',
  group: 'inline',
  inline: true,
  atom: true,
  addAttributes() {
    return {
      source: {
        default: '',
        parseHTML: (element) => element.textContent,
        rendered: false,
      },
    }
  },
  parseHTML() {
    return [{ tag: 'code[data-markdown-inline]' }]
  },
  renderHTML({ node }) {
    return [ 'code', { 'data-markdown-inline': '' }, node.attrs.source ]
  },
  renderText({ node }) {
    return node.attrs.source
  },
})

const lowlight = createLowlight(common)

// Every node and mark a page can hold. `Markdown` and this agree on what is safe to follow, so the
// editor never builds a link the page would blank.
export const markdownSchemaExtensions: AnyExtension[] = [
  StarterKit.configure({
    // Each replaced below, or left out: Markdown has no underline, and a paragraph appended to the
    // end would change a page by opening it.
    code: false,
    codeBlock: false,
    hardBreak: false,
    link: false,
    trailingNode: false,
    underline: false,
  }),
  MarkdownCode,
  MarkdownBreak,
  CodeBlockLowlight.configure({ lowlight }),
  Link.configure({
    autolink: true,
    openOnClick: false,
    // A bare `README.md` is a file, not a Moldovan website.
    shouldAutoLink: (url) => /^https?:\/\//iu.test(url),
    isAllowedUri: (url) => Boolean(url && defaultUrlTransform(url)),
  }),
  MarkdownImage.configure({ inline: true }),
  Table.configure({ resizable: false, renderWrapper: true }),
  TableRow,
  MarkdownTableHeader,
  MarkdownTableCell,
  TaskList,
  TaskItem.configure({ nested: true }),
  MarkdownBlock,
  MarkdownInline,
  MarkdownAttributes,
]

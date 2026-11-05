// Copyright © 2026 Jalapeno Labs

// eslint-disable-next-line spaced-comment -- This test alone reads files, so only it needs Node's types.
/// <reference types="node" />

import type { Node as ProseMirrorNode, Schema } from '@tiptap/pm/model'

// Core
import { readdirSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { getSchema } from '@tiptap/core'
import { Node as DocumentNode } from '@tiptap/pm/model'
import { Transform } from '@tiptap/pm/transform'
import { describe, expect, it } from 'vitest'

// Misc
import { markdownSchemaExtensions } from './editorSchema'
import { markdownCorpus } from './markdownCorpus'
import { parseMarkdown, stringifyMarkdown } from './markdownSyntax'
import { readMarkdown, writeMarkdown } from './markdownSource'
import { withMarksNested } from './nodeToMdast'

const schema: Schema = getSchema(markdownSchemaExtensions)

// This repository's own documents: real Markdown, long, and written by hand.
const DOCUMENTS_DIRECTORY = resolve(process.cwd(), '../docs')
const repositoryDocuments = readdirSync(DOCUMENTS_DIRECTORY).map((name: string): [ string, string ] => [
  name,
  readFileSync(resolve(DOCUMENTS_DIRECTORY, name), 'utf8'),
])

const corpus: [string, string][] = [
  ...Object.entries(markdownCorpus),
  ...repositoryDocuments,
]

// The server's own normalizing, which is all it does to a body.
function normalize(markdown: string) {
  return markdown.replaceAll('\r\n', '\n').trimEnd()
}

// The editor builds its document from JSON, so nothing in it is an object that was read.
function asTheEditorHoldsIt(document: ProseMirrorNode) {
  return DocumentNode.fromJSON(schema, document.toJSON())
}

// What a text means, whichever way it was spelled.
function meaningOf(markdown: string) {
  return stringifyMarkdown(withMarksNested(parseMarkdown(markdown)))
}

function blockKinds(document: ProseMirrorNode) {
  const kinds: string[] = []
  document.forEach((node) => kinds.push(node.type.name))
  return kinds
}

// The position just inside the end of the first text block named `name` that says `text`.
function endOfBlock(document: ProseMirrorNode, text: string) {
  let found: number | null = null
  document.descendants((node, position) => {
    if (found === null && node.isTextblock && node.textContent.includes(text)) {
      found = position + node.nodeSize - 1
    }
    return found === null
  })
  if (found === null) {
    throw new Error(`no text block says ${text}`)
  }
  return found
}

function appendTo(document: ProseMirrorNode, blockText: string, added: string) {
  return new Transform(document).insert(endOfBlock(document, blockText), schema.text(added)).doc
}

function removedLines(before: string, after: string) {
  const kept = new Set(after.split('\n'))
  return before.split('\n').filter((line) => !kept.has(line))
}

describe('a page opened and left alone', () => {
  it.each(corpus)('is the same bytes: %s', (_name, markdown) => {
    const body = normalize(markdown)
    const source = readMarkdown(body, schema)

    expect(writeMarkdown(source, source.document)).toBe(body)
    expect(writeMarkdown(source, asTheEditorHoldsIt(source.document))).toBe(body)
  })
})

describe('what opens as rich text', () => {
  it.each([
    [ 'headings', [ 'heading', 'heading', 'heading', 'heading', 'heading' ]],
    [ 'nestedLists', [ 'bulletList' ]],
    [ 'looseList', [ 'bulletList' ]],
    [ 'orderedLists', [ 'orderedList', 'orderedList', 'orderedList' ]],
    [ 'tableAligned', [ 'table' ]],
    [ 'tableBare', [ 'table' ]],
    [ 'tableEscapes', [ 'table' ]],
    [ 'blockquotes', [ 'blockquote', 'blockquote', 'blockquote' ]],
    [ 'codeFences', [ 'codeBlock', 'codeBlock', 'codeBlock', 'codeBlock', 'codeBlock', 'codeBlock' ]],
    [ 'specialCharacters', [ 'paragraph', 'paragraph', 'paragraph' ]],
    [ 'links', [ 'paragraph' ]],
    [ 'linkInsideBold', [ 'paragraph' ]],
    [ 'nestedEmphasis', [ 'paragraph' ]],
    [ 'emphasis', [ 'paragraph' ]],
    [ 'dangerousAddresses', [ 'paragraph' ]],
    [ 'mixedList', [ 'bulletList' ]],
    [ 'listItemStartingWithTable', [ 'bulletList' ]],
    [ 'rawHtml', [
      'markdownBlock',
      'paragraph',
      'markdownBlock',
      'markdownBlock',
      'markdownBlock',
      'paragraph',
      'markdownBlock',
    ]],
  ] as const)('%s', (name, expected) => {
    expect(blockKinds(readMarkdown(markdownCorpus[name], schema).document)).toEqual(expected)
  })

  it.each(Object.entries(markdownCorpus).filter(([ name ]) => /^(companyDoc|learning)/u.test(name)))(
    'all of %s',
    (_name, markdown) => {
      expect(blockKinds(readMarkdown(markdown, schema).document)).not.toContain('markdownBlock')
    },
  )

  it('keeps what it has no node for as text, in place', () => {
    const { document } = readMarkdown(markdownCorpus.rawHtml, schema)
    expect(document.textContent).toContain('<script>alert(1)</script>')
  })
})

describe('a page written by the editor', () => {
  it('opens as it was written, with a table in a list item it added', () => {
    const source = readMarkdown('- one', schema)
    const table = schema.nodes.table.create(null, schema.nodes.tableRow.create(null, [
      schema.nodes.tableHeader.create(null, schema.nodes.paragraph.create(null, schema.text('Term'))),
    ]))
    const item = schema.nodes.listItem.create(null, [ schema.nodes.paragraph.create(), table ])
    const list = source.document.child(0)

    const document = new Transform(source.document).insert(list.nodeSize - 1, item).doc
    const written = writeMarkdown(source, document)

    expect(written).toBe('- one\n-\n  | Term |\n  | - |')
    expect(blockKinds(readMarkdown(written, schema).document)).toEqual([ 'bulletList' ])
  })
})

describe('a whole document written anew', () => {
  it.each(corpus)('says what it said: %s', (_name, markdown) => {
    const body = normalize(markdown)
    const { document } = readMarkdown(body, schema)
    const written = writeMarkdown(readMarkdown('', schema), document)

    expect(meaningOf(written)).toBe(meaningOf(body))
  })
})

describe('an edit', () => {
  it('changes only the block it touches', () => {
    const body = markdownCorpus.specialCharacters
    const source = readMarkdown(body, schema)

    const written = writeMarkdown(source, appendTo(source.document, 'snake_case', ' Added.'))

    expect(removedLines(body, written)).toEqual([ body.split('\n')[0] ])
    expect(written).toContain('more. Added.')
  })

  it('keeps the spelling of a block it did not touch, whatever the block', () => {
    const body = '* star\n\nA paragraph.\n\nName | Role\n--- | ---\nAlice | Lead\n\nSetext\n===\n\n<b>raw</b>'
    const source = readMarkdown(body, schema)

    const written = writeMarkdown(source, appendTo(source.document, 'A paragraph.', ' More.'))

    expect(written).toBe(body.replace('A paragraph.', 'A paragraph. More.'))
  })

  it('keeps the blank lines between blocks it did not touch', () => {
    const body = 'One.\n\n\n\nTwo.\n\n\nThree.\nStill three.'
    const source = readMarkdown(body, schema)

    expect(writeMarkdown(source, appendTo(source.document, 'Two.', ' More.')))
      .toBe('One.\n\n\n\nTwo. More.\n\n\nThree.\nStill three.')
  })

  it('needs no needless escapes in the block it touches', () => {
    const source = readMarkdown('snake_case, ~/.config, arr[0], and 2 * 3.', schema)

    expect(writeMarkdown(source, appendTo(source.document, 'snake_case', ' More.')))
      .toBe('snake_case, ~/.config, arr[0], and 2 * 3. More.')
  })

  it('leaves one escape of a pair, when the other is all it takes', () => {
    const source = readMarkdown('\\*not bold\\*', schema)

    expect(writeMarkdown(source, appendTo(source.document, 'not bold', ' More.'))).toBe('\\*not bold* More.')
  })

  it('keeps an escape that means something', () => {
    const source = readMarkdown('\\[not a link\\](/x)', schema)

    expect(writeMarkdown(source, appendTo(source.document, 'not a link', ' More.'))).toBe('\\[not a link](/x) More.')
  })

  it('keeps brackets escaped where a definition on the page would make them a link', () => {
    const body = 'Text with \\[term].\n\n[term]: https://example.com'
    const source = readMarkdown(body, schema)

    expect(writeMarkdown(source, appendTo(source.document, 'Text with', ' More.')))
      .toBe('Text with \\[term]. More.\n\n[term]: https://example.com')
  })

  it('keeps a hard-wrapped paragraph on its lines', () => {
    const source = readMarkdown('First line\nsecond line\nthird line', schema)

    expect(writeMarkdown(source, appendTo(source.document, 'First line', ' more')))
      .toBe('First line\nsecond line\nthird line more')
  })

  it('touches one item of a list', () => {
    const body = '* one\n* two\n\n  still two\n* three'
    const source = readMarkdown(body, schema)

    expect(writeMarkdown(source, appendTo(source.document, 'three', ' more')))
      .toBe('* one\n* two\n\n  still two\n* three more')
  })

  it('touches one row of a table, and leaves the padding of the rest', () => {
    const body = markdownCorpus.tableAligned
    const source = readMarkdown(body, schema)

    const written = writeMarkdown(source, appendTo(source.document, 'longest text', ' more'))

    expect(written.split('\n').slice(0, 3)).toEqual(body.split('\n').slice(0, 3))
    expect(written.split('\n')[3]).toBe('| long | longer | longest text more | e |')
  })

  it('renumbers the items after one added to an ordered list', () => {
    const body = '1. one\n2. two\n3. three'
    const source = readMarkdown(body, schema)
    const list = source.document.child(0)
    const added = list.type.schema.nodes.listItem.create(
      null,
      schema.nodes.paragraph.create(null, schema.text('new')),
    )

    const document = new Transform(source.document).insert(list.child(0).nodeSize + 1, added).doc

    expect(writeMarkdown(source, document)).toBe('1. one\n2. new\n3. two\n4. three')
  })

  it('keeps a list numbered with ones numbered with ones', () => {
    const source = readMarkdown('1. one\n1. two\n1. three', schema)

    expect(writeMarkdown(source, appendTo(source.document, 'three', ' more')))
      .toBe('1. one\n1. two\n1. three more')
  })

  it('writes a new item with the marker of the list it joined', () => {
    const source = readMarkdown('* one\n* two', schema)
    const list = source.document.child(0)
    const added = schema.nodes.listItem.create(null, schema.nodes.paragraph.create(null, schema.text('new')))

    const document = new Transform(source.document).insert(list.nodeSize - 1, added).doc

    expect(writeMarkdown(source, document)).toBe('* one\n* two\n* new')
  })

  it('keeps text out of a code fence nobody closed', () => {
    const source = readMarkdown(markdownCorpus.unclosedFence, schema)
    const paragraph = schema.nodes.paragraph.create(null, schema.text('Added after.'))

    const document = new Transform(source.document).insert(source.document.content.size, paragraph).doc
    const written = writeMarkdown(source, document)

    expect(blockKinds(readMarkdown(written, schema).document)).toEqual([ 'paragraph', 'codeBlock', 'paragraph' ])
  })

  it('drops a blank paragraph, which Markdown cannot say', () => {
    const source = readMarkdown('One.', schema)
    const blank = schema.nodes.paragraph.create()

    const document = new Transform(source.document).insert(source.document.content.size, blank).doc

    expect(writeMarkdown(source, document)).toBe('One.')
  })

  it('moves the spaces at the edge of emphasis out of it', () => {
    const bold = schema.marks.bold.create()
    const document = schema.nodes.doc.create(null, schema.nodes.paragraph.create(null, [
      schema.text('Some '),
      schema.text(' bold ', [ bold ]),
      schema.text('words.'),
    ]))

    expect(writeMarkdown(readMarkdown('', schema), document)).toBe('Some  **bold** words.')
  })

  it('drops the spaces at the edges of a line, which Markdown would encode', () => {
    const document = schema.nodes.doc.create(null, schema.nodes.paragraph.create(null, schema.text(' Spaced ')))

    expect(writeMarkdown(readMarkdown('', schema), document)).toBe('Spaced')
  })

  it('escapes a pipe in a new table cell', () => {
    const source = readMarkdown('| a |\n| - |\n| b |', schema)

    expect(writeMarkdown(source, appendTo(source.document, 'b', ' | c'))).toBe('| a |\n| - |\n| b \\| c |')
  })
})

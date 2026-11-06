// Copyright © 2026 Jalapeno Labs

import type { EditorState, Transaction } from '@tiptap/pm/state'
import type { EditorView } from '@tiptap/pm/view'

// Core
import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'

// The file's place while it is on its way, which only the editor sees. Nothing of it reaches the
// document, so the Markdown never holds a half-finished upload.
type PlaceholderAction =
  | { kind: 'add', id: object, position: number, label: string }
  | { kind: 'remove', id: object }

const placeholdersKey = new PluginKey<DecorationSet>('attachmentPlaceholders')

function drawPlaceholder(label: string) {
  const placeholder = document.createElement('span')
  placeholder.className = 'markdown-editor-uploading'
  placeholder.textContent = label
  return placeholder
}

function applyAction(placeholders: DecorationSet, action: PlaceholderAction, transaction: Transaction) {
  if (action.kind === 'remove') {
    return placeholders.remove(placeholders.find(undefined, undefined, (spec) => spec.id === action.id))
  }
  const widget = Decoration.widget(action.position, () => drawPlaceholder(action.label), { id: action.id })
  return placeholders.add(transaction.doc, [ widget ])
}

// Keeps each file's place as the text around it changes, as ProseMirror's own upload example does:
// the person keeps typing while a file uploads, so the position it was dropped at would soon be
// wrong, and a decoration moves with the document.
export const AttachmentPlaceholders = Extension.create({
  name: 'attachmentPlaceholders',

  addProseMirrorPlugins() {
    return [
      new Plugin<DecorationSet>({
        key: placeholdersKey,
        state: {
          init: () => DecorationSet.empty,
          apply(transaction, placeholders) {
            const moved = placeholders.map(transaction.mapping, transaction.doc)
            const action: PlaceholderAction | undefined = transaction.getMeta(placeholdersKey)
            return action
              ? applyAction(moved, action, transaction)
              : moved
          },
        },
        props: {
          decorations: (state) => placeholdersKey.getState(state),
        },
      }),
    ]
  },
})

export function placeAttachmentPlaceholder(view: EditorView, id: object, position: number, label: string) {
  const action: PlaceholderAction = { kind: 'add', id, position, label }
  view.dispatch(view.state.tr.setMeta(placeholdersKey, action))
}

export function removeAttachmentPlaceholder(view: EditorView, id: object) {
  const action: PlaceholderAction = { kind: 'remove', id }
  view.dispatch(view.state.tr.setMeta(placeholdersKey, action))
}

// Where the placeholder `id` sits now; null when what held it was deleted, and it with it.
export function findAttachmentPlaceholder(state: EditorState, id: object) {
  const placeholders = placeholdersKey.getState(state)
  const [ found ] = placeholders?.find(undefined, undefined, (spec) => spec.id === id) ?? []
  return found?.from ?? null
}

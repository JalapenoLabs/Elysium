// Copyright © 2026 Jalapeno Labs

import type { ChainedCommands, Editor } from '@tiptap/core'
import type { IconType } from 'react-icons'

// User interface
import {
  LuCode,
  LuHeading1,
  LuHeading2,
  LuHeading3,
  LuImage,
  LuList,
  LuListChecks,
  LuListOrdered,
  LuMinus,
  LuPilcrow,
  LuTable,
  LuTextQuote,
} from 'react-icons/lu'

type EditorCommandId =
  | 'paragraph'
  | 'heading1'
  | 'heading2'
  | 'heading3'
  | 'bulletList'
  | 'orderedList'
  | 'taskList'
  | 'quote'
  | 'codeBlock'
  | 'table'
  | 'divider'
  | 'image'

export type EditorCommand = {
  // Names its label and its search words in the translations.
  id: EditorCommandId
  icon: IconType
  // What it does, added to a chain so a slash command can first remove what was typed. Without one,
  // it needs asking first, which only the image does.
  apply?: (chain: ChainedCommands) => ChainedCommands
  isActive?: (editor: Editor) => boolean
}

// The blocks a page can add, for the toolbar and the slash menu alike.
export const editorCommands: EditorCommand[] = [
  {
    id: 'paragraph',
    icon: LuPilcrow,
    apply: (chain) => chain.setParagraph(),
  },
  {
    id: 'heading1',
    icon: LuHeading1,
    apply: (chain) => chain.toggleHeading({ level: 1 }),
    isActive: (editor) => editor.isActive('heading', { level: 1 }),
  },
  {
    id: 'heading2',
    icon: LuHeading2,
    apply: (chain) => chain.toggleHeading({ level: 2 }),
    isActive: (editor) => editor.isActive('heading', { level: 2 }),
  },
  {
    id: 'heading3',
    icon: LuHeading3,
    apply: (chain) => chain.toggleHeading({ level: 3 }),
    isActive: (editor) => editor.isActive('heading', { level: 3 }),
  },
  {
    id: 'bulletList',
    icon: LuList,
    apply: (chain) => chain.toggleBulletList(),
    isActive: (editor) => editor.isActive('bulletList'),
  },
  {
    id: 'orderedList',
    icon: LuListOrdered,
    apply: (chain) => chain.toggleOrderedList(),
    isActive: (editor) => editor.isActive('orderedList'),
  },
  {
    id: 'taskList',
    icon: LuListChecks,
    apply: (chain) => chain.toggleTaskList(),
    isActive: (editor) => editor.isActive('taskList'),
  },
  {
    id: 'quote',
    icon: LuTextQuote,
    apply: (chain) => chain.toggleBlockquote(),
    isActive: (editor) => editor.isActive('blockquote'),
  },
  {
    id: 'codeBlock',
    icon: LuCode,
    apply: (chain) => chain.toggleCodeBlock(),
    isActive: (editor) => editor.isActive('codeBlock'),
  },
  {
    id: 'table',
    icon: LuTable,
    apply: (chain) => chain.insertTable({ rows: 3, cols: 3, withHeaderRow: true }),
  },
  {
    id: 'divider',
    icon: LuMinus,
    apply: (chain) => chain.setHorizontalRule(),
  },
  {
    id: 'image',
    icon: LuImage,
  },
]

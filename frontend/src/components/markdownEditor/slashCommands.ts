// Copyright © 2026 Jalapeno Labs

import type { Range } from '@tiptap/core'
import type { SuggestionProps } from '@tiptap/suggestion'

// Core
import { Extension } from '@tiptap/core'
import { PluginKey } from '@tiptap/pm/state'
import { Suggestion } from '@tiptap/suggestion'

export type SlashMenuState = {
  // What follows the `/`.
  query: string
  // The `/` and what follows it, which a chosen command replaces.
  range: Range
  // Where the caret is, which the menu opens beside.
  anchor: DOMRect | null
}

type Options = {
  onChange: (state: SlashMenuState | null) => void
  // Keys the open menu takes from the editor, answering whether it used them.
  onKeyDown: (event: KeyboardEvent) => boolean
}

function readState(props: SuggestionProps): SlashMenuState {
  return {
    query: props.query,
    range: props.range,
    anchor: props.clientRect?.() ?? null,
  }
}

// Typing `/` after a space or at a line's start opens the slash menu. The menu itself is React
// (`SlashMenu`), so this only reports what was typed and passes keys on. Never inside code, where
// a slash is a slash.
export const SlashCommands = Extension.create<Options>({
  name: 'slashCommands',

  addOptions() {
    return {
      onChange: () => undefined,
      onKeyDown: () => false,
    }
  },

  addProseMirrorPlugins() {
    const { onChange, onKeyDown } = this.options

    return [
      // eslint-disable-next-line new-cap -- Tiptap's own name for a plain function.
      Suggestion({
        editor: this.editor,
        pluginKey: new PluginKey('slashCommands'),
        char: '/',
        allow: ({ state, range }) => !state.doc.resolve(range.from).parent.type.spec.code,
        render: () => ({
          onStart: (props) => onChange(readState(props)),
          onUpdate: (props) => onChange(readState(props)),
          onExit: () => onChange(null),
          onKeyDown: ({ event }) => onKeyDown(event),
        }),
      }),
    ]
  },
})

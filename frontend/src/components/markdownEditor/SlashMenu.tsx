// Copyright © 2026 Jalapeno Labs

import type { RefObject } from 'react'
import type { EditorCommand } from './editorCommands'
import type { SlashMenuState } from './slashCommands'

// Core
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'

// Misc
import { editorCommands } from './editorCommands'

type Props = {
  state: SlashMenuState
  // Where the editor sends the keys the open menu takes.
  keysRef: RefObject<((event: KeyboardEvent) => boolean) | null>
  onChoose: (command: EditorCommand) => void
}

const MENU_WIDTH_PX = 256
const MENU_MAX_HEIGHT_PX = 288
const ANCHOR_GAP_PX = 6
const VIEWPORT_MARGIN_PX = 8

// Below the caret, or above it when it would run off the screen, and always on the screen, as on a
// phone, where the caret can be near the right edge.
function placeMenu(anchor: DOMRect | null) {
  if (!anchor) {
    return { top: VIEWPORT_MARGIN_PX, left: VIEWPORT_MARGIN_PX }
  }
  const fitsBelow = anchor.bottom + ANCHOR_GAP_PX + MENU_MAX_HEIGHT_PX < window.innerHeight
  return {
    top: fitsBelow
      ? anchor.bottom + ANCHOR_GAP_PX
      : Math.max(VIEWPORT_MARGIN_PX, anchor.top - ANCHOR_GAP_PX - MENU_MAX_HEIGHT_PX),
    left: Math.max(VIEWPORT_MARGIN_PX, Math.min(anchor.left, window.innerWidth - MENU_WIDTH_PX - VIEWPORT_MARGIN_PX)),
  }
}

// What `/` opens: the blocks a page can add, narrowed by what follows it. Focus stays in the editor,
// so typing goes on narrowing it, and it answers the arrow keys and Enter through `keysRef`. A
// list box to `readPageShortcut`, so Esc closes it rather than leaving the page.
export function SlashMenu(props: Props) {
  const { t } = useTranslation('common')
  const [ highlighted, setHighlighted ] = useState(0)
  const query = props.state.query.trim().toLowerCase()

  const matches = editorCommands.filter((command) => {
    const words = `${t(`markdownEditor.commands.${command.id}`)} ${t(`markdownEditor.tags.${command.id}`)}`
    return words.toLowerCase().includes(query)
  })
  const active = Math.min(highlighted, Math.max(matches.length - 1, 0))

  useEffect(() => {
    const { keysRef, onChoose } = props
    keysRef.current = (event) => {
      if (event.key === 'ArrowDown' && matches.length) {
        setHighlighted((active + 1) % matches.length)
        return true
      }
      if (event.key === 'ArrowUp' && matches.length) {
        setHighlighted((active - 1 + matches.length) % matches.length)
        return true
      }
      if (event.key === 'Enter' || event.key === 'Tab') {
        if (!matches.length) {
          return false
        }
        onChoose(matches[active])
        return true
      }
      return false
    }
    return () => {
      keysRef.current = null
    }
  })

  return createPortal(
    <div
      role='listbox'
      aria-label={t('markdownEditor.slashLabel')}
      className='markdown-editor-slash-menu'
      style={placeMenu(props.state.anchor)}
    >
      {matches.map((command, index) => {
        const Icon = command.icon
        return <button
          key={command.id}
          type='button'
          role='option'
          aria-selected={index === active}
          ref={index === active
            ? (element) => element?.scrollIntoView({ block: 'nearest' })
            : undefined}
          // The press must not take focus from the editor, whose selection the command acts on.
          onMouseDown={(event) => event.preventDefault()}
          onMouseEnter={() => setHighlighted(index)}
          onClick={() => props.onChoose(command)}
        >
          <Icon className='size-4 shrink-0' aria-hidden />
          <span>{t(`markdownEditor.commands.${command.id}`)}</span>
        </button>
      })}
      {!matches.length && <p className='px-2 py-1.5 text-sm opacity-70'>{t('markdownEditor.noCommands')}</p>}
    </div>,
    document.body,
  )
}

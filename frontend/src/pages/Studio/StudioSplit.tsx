// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { Layout } from 'react-resizable-panels'
import type { StudioLayout, StudioLayoutMode } from './studioLayout'

// Core
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Group, Panel, Separator, useGroupRef } from 'react-resizable-panels'

// Misc
import { CHAT_PANEL_ID, getPanelLayout, MIN_PANEL_PERCENT, PREVIEW_PANEL_ID } from './studioLayout'

// Stacked, the stage sits above the conversation when both show, and takes the whole height
// alone. Hidden columns stay mounted, so switching keeps the model loaded and the
// conversation's scroll position.
const stackedPreviewClassNames = {
  both: 'h-[45%] shrink-0 border-b border-separator',
  preview: 'min-h-0 flex-1',
  chat: 'hidden',
} as const satisfies Record<StudioLayoutMode, string>

const stackedChatClassNames = {
  both: 'min-h-0 flex-1',
  preview: 'hidden',
  chat: 'min-h-0 flex-1',
} as const satisfies Record<StudioLayoutMode, string>

type Props = {
  layout: StudioLayout
  isStacked: boolean
  // The user dragged the split, possibly closing a column.
  onResize: (panels: Layout) => void
  preview: ReactNode
  chat: ReactNode
}

// The item page's two columns: side by side with a draggable split, or stacked on narrow
// screens.
export function StudioSplit(props: Props) {
  const { t } = useTranslation('studio')
  const groupRef = useGroupRef()
  const layout = props.layout

  // The mode control moves the panels; a drag already has, so applying it again is a no-op.
  useEffect(() => {
    groupRef.current?.setLayout(getPanelLayout(layout))
  }, [ groupRef, layout ])

  if (props.isStacked) {
    return <div className='flex h-full flex-col'>
      <div className={stackedPreviewClassNames[layout.mode]}>{props.preview}</div>
      <div className={stackedChatClassNames[layout.mode]}>{props.chat}</div>
    </div>
  }

  return <Group
    orientation='horizontal'
    groupRef={groupRef}
    defaultLayout={getPanelLayout(layout)}
    onLayoutChanged={(panels, meta) => {
      if (meta.isUserInteraction) {
        props.onResize(panels)
      }
    }}
    className='h-full'
  >
    <Panel
      id={PREVIEW_PANEL_ID}
      collapsible
      minSize={`${MIN_PANEL_PERCENT}%`}
      className='h-full'
    >
      {props.preview}
    </Panel>
    <Separator
      aria-label={t('layout.resize')}
      className='w-1 bg-separator transition-colors hover:bg-accent focus-visible:bg-accent focus-visible:outline-none'
    />
    <Panel
      id={CHAT_PANEL_ID}
      collapsible
      minSize={`${MIN_PANEL_PERCENT}%`}
      className='h-full'
    >
      {props.chat}
    </Panel>
  </Group>
}

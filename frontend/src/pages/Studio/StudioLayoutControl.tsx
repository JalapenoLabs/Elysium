// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { IconType } from 'react-icons'
import type { StudioLayoutMode } from './studioLayout'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { ToggleButton, ToggleButtonGroup } from '@heroui/react'
import { LuColumns2, LuImage, LuMessagesSquare } from 'react-icons/lu'

// Misc
import { STUDIO_LAYOUT_MODES } from './studioLayout'

const labelKeys = {
  both: 'layout.both',
  preview: 'layout.preview',
  chat: 'layout.chat',
} as const satisfies Record<StudioLayoutMode, string>

const icons = {
  both: LuColumns2,
  preview: LuImage,
  chat: LuMessagesSquare,
} as const satisfies Record<StudioLayoutMode, IconType>

type Props = {
  mode: StudioLayoutMode
  onChange: (mode: StudioLayoutMode) => void
}

// Both, Preview, or Chat. Exactly one is always chosen, so both columns can never be hidden.
export function StudioLayoutControl(props: Props) {
  const { t } = useTranslation('studio')

  function changeMode(keys: Set<Key>) {
    // The group disallows an empty selection, so one mode is always chosen.
    const [ key ] = [ ...keys ]
    const mode = STUDIO_LAYOUT_MODES.find((candidate) => candidate === key)
    if (!mode) {
      console.debug('StudioLayoutControl ignored an unknown mode', { key })
      return
    }
    props.onChange(mode)
  }

  return <ToggleButtonGroup
    size='sm'
    aria-label={t('layout.label')}
    selectionMode='single'
    disallowEmptySelection
    selectedKeys={[ props.mode ]}
    onSelectionChange={changeMode}
  >{
      STUDIO_LAYOUT_MODES.map((mode, index) => {
        const Icon = icons[mode]
        return <ToggleButton key={mode} id={mode} aria-label={t(labelKeys[mode])}>
          {index > 0 && <ToggleButtonGroup.Separator />}
          <Icon className='size-4' aria-hidden />
          <span>{t(labelKeys[mode])}</span>
        </ToggleButton>
      })
    }</ToggleButtonGroup>
}

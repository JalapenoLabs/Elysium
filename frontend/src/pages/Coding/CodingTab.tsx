// Copyright © 2026 Jalapeno Labs

import type { IDockviewPanelHeaderProps } from 'dockview-react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'

// Every panel's tab on the Coding page: its title and a close button. Dockview's own tab
// names its close button in English and gives it no tooltip; this one is Dockview's tab in
// every other way, its classes included, so it looks and drags the same.
export function CodingTab(props: IDockviewPanelHeaderProps) {
  const { t } = useTranslation('coding')
  const [ title, setTitle ] = useState(props.api.title ?? '')

  useEffect(() => {
    const subscription = props.api.onDidTitleChange((event) => setTitle(event.title))
    return () => subscription.dispose()
  }, [ props.api ])

  const closeLabel = t('panels.close', { title })

  return <div className='dv-default-tab'>
    <span className='dv-default-tab-content'>{title}</span>
    <Tooltip delay={300}>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        className='coding-tab-close size-6 min-w-0'
        aria-label={closeLabel}
        onPress={() => props.api.close()}
      >
        <LuX className='size-3.5' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{closeLabel}</span>
      </Tooltip.Content>
    </Tooltip>
  </div>
}

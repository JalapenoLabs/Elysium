// Copyright © 2026 Jalapeno Labs

import type { Ref } from 'react'
import type { PageSave } from './pageSave'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'

// Misc
import { canPressSave } from './pageSave'

type Props = {
  save: PageSave
  isSaving: boolean
  onPress: () => void
  // The header's button is watched, so the floating card shows once it scrolls away.
  ref?: Ref<HTMLDivElement>
}

// A page's Save: it runs the page's save, and says why when it cannot.
export function PageSaveButton(props: Props) {
  const { t } = useTranslation('common')
  const save = props.save

  let disabledReason: string | null = null
  if (save.blockedReason) {
    disabledReason = save.blockedReason
  }
  else if (!save.isDirty) {
    disabledReason = t('page.save.nothingChanged')
  }

  // A disabled button fires no hover events, so the tooltip's trigger is the wrapper. It is
  // itself focusable, so it wraps the button only while there is a reason to show.
  const button = <div ref={props.ref}>
    <Button
      isDisabled={!canPressSave(save, props.isSaving)}
      isPending={props.isSaving}
      onPress={props.onPress}
    >
      <span>{save.label ?? t('actions.save')}</span>
    </Button>
  </div>

  if (!disabledReason || props.isSaving) {
    return button
  }
  return <Tooltip delay={200}>
    <Tooltip.Trigger>{
      button
    }</Tooltip.Trigger>
    <Tooltip.Content className='max-w-xs'>
      <span>{disabledReason}</span>
    </Tooltip.Content>
  </Tooltip>
}

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
  // The header's button is watched, so the floating card shows once it scrolls away.
  ref?: Ref<HTMLDivElement>
}

// A page's Save: it submits the page's form, and says why when it cannot.
export function PageSaveButton(props: Props) {
  const { t } = useTranslation('common')

  const button = <div ref={props.ref}>
    <Button
      type='submit'
      form={props.save.formId}
      isDisabled={!canPressSave(props.save)}
      isPending={props.save.isPending}
    >
      <span>{props.save.label ?? t('actions.save')}</span>
    </Button>
  </div>

  // The tooltip's trigger is itself focusable, so it wraps the button only while there is a
  // reason to show. A disabled button fires no hover events, so the trigger is the wrapper.
  if (!props.save.blockedReason) {
    return button
  }
  return <Tooltip delay={200}>
    <Tooltip.Trigger>{
      button
    }</Tooltip.Trigger>
    <Tooltip.Content className='max-w-xs'>
      <span>{props.save.blockedReason}</span>
    </Tooltip.Content>
  </Tooltip>
}

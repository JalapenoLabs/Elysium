// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Popover, Tooltip } from '@heroui/react'
import { LuCircleHelp } from 'react-icons/lu'

type Props = {
  children: ReactNode
  // Names the button, for its tooltip and screen readers; "Help" unless given.
  label?: string
}

// An icon-only `?` that opens an explanation in a popover, for the rare thing that needs one
// (`~/.claude/docs/ux.md`, Brevity): beside a page's title (`Page`'s `help`), or beside a
// message whose detail only some people need.
export function HelpPopover(props: Props) {
  const { t } = useTranslation('common')
  const label = props.label ?? t('page.help')

  return <Popover>
    <Tooltip delay={300}>
      <Button isIconOnly size='sm' variant='ghost' aria-label={label}>
        <LuCircleHelp className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{label}</span>
      </Tooltip.Content>
    </Tooltip>
    <Popover.Content placement='bottom start' className='max-w-sm'>
      <Popover.Dialog className='text-sm'>{
        props.children
      }</Popover.Dialog>
    </Popover.Content>
  </Popover>
}

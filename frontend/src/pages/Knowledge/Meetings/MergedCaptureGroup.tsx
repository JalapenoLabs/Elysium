// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { CaptureGroup } from './meetingCaptures'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Link, Tooltip } from '@heroui/react'
import { LuSplit } from 'react-icons/lu'

// Misc
import { getMeetingUrl } from '../../../urls'

type Props = {
  from: NonNullable<CaptureGroup['from']>
  canSplit: boolean
  isSplitting: boolean
  // Another group is splitting out, and one runs at a time.
  isDisabled: boolean
  onSplit: () => void
  // Its captures' rows.
  children: ReactNode
}

// The captures one merge brought into a meeting, boxed under a link to the meeting they came from
// and Split out, which makes that meeting its own again with all of them (docs/meetings.md, The
// same meeting twice, Splitting).
export function MergedCaptureGroup(props: Props) {
  const { t } = useTranslation('meetings')
  const label = props.from.title
    ? t('review.captures.from', { title: props.from.title })
    : t('review.captures.fromUntitled')

  // Split out is disabled while a capture of it was moved by hand, and while another group splits.
  const isButtonDisabled = !props.canSplit || props.isDisabled
  const button = <Button
    size='sm'
    variant='outline'
    isDisabled={isButtonDisabled}
    isPending={props.isSplitting}
    onPress={props.onSplit}
  >
    <LuSplit className='size-4' aria-hidden />
    <span>{t('review.captures.split')}</span>
  </Button>
  const tip = <Tooltip.Content className='max-w-xs break-normal'>
    <span>{props.canSplit
      ? t('review.captures.splitHint')
      : t('review.captures.splitBlocked')}</span>
  </Tooltip.Content>

  // The meeting's name has the box's whole width, since the lane is narrow, and Split out sits at
  // its foot, under every capture it moves. A disabled button fires no hover, so while it is one the
  // tooltip's trigger wraps it, as `PageSaveButton` does; the trigger is a tab stop of its own, so it
  // is there only then.
  return <section aria-label={label} className='rounded-lg border border-border p-2'>
    <p className='compact text-xs'>
      <Link href={getMeetingUrl(props.from.meetingId)} className='text-link'>
        <span className='break-words'>{label}</span>
      </Link>
    </p>
    {props.children}
    <div className='mt-2 flex justify-end'>{
      isButtonDisabled
        ? <Tooltip delay={300}>
          <Tooltip.Trigger>
            <div>{button}</div>
          </Tooltip.Trigger>
          {tip}
        </Tooltip>
        : <Tooltip delay={300}>
          {button}
          {tip}
        </Tooltip>
    }</div>
  </section>
}

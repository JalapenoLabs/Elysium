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

  const button = <Button
    size='sm'
    variant='outline'
    isDisabled={!props.canSplit || props.isDisabled}
    isPending={props.isSplitting}
    onPress={props.onSplit}
  >
    <LuSplit className='size-4' aria-hidden />
    <span>{t('review.captures.split')}</span>
  </Button>

  // The meeting's name has the box's whole width, since the lane is narrow, and Split out sits at
  // its foot, under every capture it moves.
  return <section aria-label={label} className='rounded-lg border border-border p-2'>
    <p className='compact text-xs'>
      <Link href={getMeetingUrl(props.from.meetingId)} className='text-link'>
        <span className='break-words'>{label}</span>
      </Link>
    </p>
    {props.children}
    <div className='mt-2 flex justify-end'>{
      props.canSplit
        ? <Tooltip delay={300}>
          {button}
          <Tooltip.Content className='max-w-xs break-normal'>{t('review.captures.splitHint')}</Tooltip.Content>
        </Tooltip>
        // A disabled button fires no hover, so the tooltip's trigger wraps it to say why.
        : <Tooltip delay={200}>
          <Tooltip.Trigger>
            <div>{button}</div>
          </Tooltip.Trigger>
          <Tooltip.Content className='max-w-xs break-normal'>
            <span>{t('review.captures.splitBlocked')}</span>
          </Tooltip.Content>
        </Tooltip>
    }</div>
  </section>
}

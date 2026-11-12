// Copyright © 2026 Jalapeno Labs

import type { MeetingAttachment } from '../../../api/routes/meetingRoutes'
import type { PlacedPin } from './meetingAttachments'
import type { WordRange } from './meetingTranscript'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label, Tooltip } from '@heroui/react'
import { LuChevronDown, LuFile, LuImage, LuPin, LuPinOff } from 'react-icons/lu'

type Props = {
  range: WordRange
  pins: PlacedPin[]
  attachments: MeetingAttachment[]
  // Pins one over the excerpt's words; null where the meeting takes no change.
  onPin: ((attachmentId: string) => void) | null
  // Removes one pin, which its toast's Undo puts back; null where the meeting takes no change.
  onUnpin: ((markId: string) => void) | null
}

// The files and frames an excerpt carries to its projects (docs/meeting-review.md, Attachments and
// frames): each pinned over its words, with Unpin, and Pin a file or frame for the meeting's others,
// which pins one over the excerpt's whole range.
export function ExcerptPins(props: Props) {
  const { t } = useTranslation('meetings')
  const byId = new Map(props.attachments.map((attachment) => [ attachment.id, attachment ]))
  const pinned = props.pins.filter((pin) => pin.range.start <= props.range.end
    && props.range.start <= pin.range.end
    && byId.has(pin.attachmentId))
  const pinnedIds = new Set(pinned.map((pin) => pin.attachmentId))
  const others = props.attachments.filter((attachment) => !pinnedIds.has(attachment.id))
  const { onPin, onUnpin } = props

  const labelOf = (attachment: MeetingAttachment) => attachment.kind === 'frame'
    ? attachment.caption ?? attachment.fileName
    : attachment.fileName

  return <div className='flex flex-col gap-2'>
    <p className='text-xs opacity-70'>{t('review.files.pinnedHere')}</p>
    {!pinned.length && <p className='text-xs opacity-60'>{t('review.files.nothingPinned')}</p>}
    {Boolean(pinned.length) && <ul className='flex flex-col gap-1'>{
      pinned.map((pin) => {
        const attachment = byId.get(pin.attachmentId)
        if (!attachment) {
          return null
        }
        return <li key={pin.markId} className='flex items-center gap-2'>
          {attachment.displaysInline && attachment.address
            ? <img
              src={attachment.address}
              alt=''
              loading='lazy'
              className='aspect-video w-16 shrink-0 rounded bg-black object-contain'
            />
            : <LuFile className='size-4 shrink-0 opacity-60' aria-hidden />}
          <span className='min-w-0 flex-1 truncate text-xs'>{labelOf(attachment)}</span>
          {onUnpin && <Tooltip delay={300}>
            <Button
              isIconOnly
              size='sm'
              variant='ghost'
              aria-label={t('review.files.unpin', { name: labelOf(attachment) })}
              onPress={() => onUnpin(pin.markId)}
            >
              <LuPinOff className='size-4' aria-hidden />
            </Button>
            <Tooltip.Content>{t('review.files.unpin', { name: labelOf(attachment) })}</Tooltip.Content>
          </Tooltip>}
        </li>
      })
    }</ul>}
    {onPin && Boolean(others.length) && <Dropdown>
      <Button size='sm' variant='outline' className='self-start'>
        <LuPin className='size-4' aria-hidden />
        <span>{t('review.files.pinHere')}</span>
        <LuChevronDown className='size-4' aria-hidden />
      </Button>
      <Dropdown.Popover placement='bottom start' className='max-h-80 overflow-y-auto'>
        <Dropdown.Menu
          aria-label={t('review.files.pinHereLabel')}
          onAction={(key) => onPin(String(key))}
        >{
            others.map((attachment) => <Dropdown.Item
              key={attachment.id}
              id={attachment.id}
              textValue={labelOf(attachment)}
            >
              {attachment.kind === 'frame'
                ? <LuImage className='size-4' aria-hidden />
                : <LuFile className='size-4' aria-hidden />}
              <Label className='max-w-64 truncate'>{labelOf(attachment)}</Label>
            </Dropdown.Item>)
          }</Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>}
  </div>
}

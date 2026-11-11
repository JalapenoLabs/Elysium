// Copyright © 2026 Jalapeno Labs

import type { CSSProperties, RefObject } from 'react'

// Core
import { useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Kbd, ToggleButton, ToggleButtonGroup, Tooltip } from '@heroui/react'
import { LuMessageSquarePlus, LuPencil, LuPlay, LuTextSelect, LuX } from 'react-icons/lu'

export type MarkPill = {
  markId: string
  label: string
}

type Props = {
  // The transcript the bar floats in, and the word it floats beside.
  containerRef: RefObject<HTMLDivElement | null>
  anchorIndex: number
  // Every mark under the word clicked, shortest first, the selected one pressed.
  pills: MarkPill[]
  selectedMarkId: string | null
  onSelectMark: (markId: string) => void
  // Whether words are selected, which Edit and Comment act on.
  hasSelection: boolean
  onEdit: () => void
  onComment: () => void
  onClear: () => void
  // On a touch screen, which has no Shift: a moment picked offers to start a selection there,
  // which the next tap ends.
  onSelectFromHere: (() => void) | null
  isExtending: boolean
  // Plays the meeting from what is picked, while it has a recording to play.
  onPlayFromHere: (() => void) | null
}

type Position = {
  top: number
  // From the left edge for a word in the left half, else from the right, so the bar never runs
  // off the transcript.
  left: number | null
  right: number | null
  isBelow: boolean
}

// How close to the top of the transcript a word may be before the bar goes below it instead.
const ROOM_ABOVE_PX = 56
const GAP_PX = 8

// The bar takes the width its actions need, so an edge it is held to gives way as far as the
// transcript's other edge rather than wrapping the actions onto lines of their own.
function measure(container: HTMLDivElement, anchorIndex: number, barWidth: number): Position | null {
  const word = container.querySelector(`[data-word-index="${anchorIndex}"]`)
  if (!word) {
    return null
  }
  const box = container.getBoundingClientRect()
  const rect = word.getBoundingClientRect()
  const top = rect.top - box.top
  const isBelow = top < ROOM_ABOVE_PX
  const isLeftHalf = rect.left - box.left < box.width / 2
  const furthest = Math.max(0, box.width - barWidth)
  return {
    top: isBelow
      ? rect.bottom - box.top + GAP_PX
      : top - GAP_PX,
    left: isLeftHalf
      ? Math.min(furthest, Math.max(0, rect.left - box.left))
      : null,
    right: isLeftHalf
      ? null
      : Math.min(furthest, Math.max(0, box.right - rect.right)),
    isBelow,
  }
}

// The bar beside what is picked in a meeting's transcript (docs/meeting-review.md, Clicking,
// selecting, and acting): the marks under the word clicked as pills, which Tab moves between, then
// what can be done to the words selected, each with its key, and Play from here while the meeting
// has a recording. Actions whose features are not built yet (Assign to a project, Black out, Pin
// a frame) join it with them.
export function TranscriptFloatingBar(props: Props) {
  const { t } = useTranslation('meetings')
  const [ position, setPosition ] = useState<Position | null>(null)
  const barRef = useRef<HTMLDivElement>(null)
  const { containerRef, anchorIndex } = props

  // The word moves when the text around it reflows, so the bar follows the transcript's size, and
  // its own as actions come and go.
  useLayoutEffect(() => {
    const container = containerRef.current
    if (!container) {
      return undefined
    }
    const place = () => setPosition(measure(container, anchorIndex, barRef.current?.offsetWidth ?? 0))
    place()
    const observer = new ResizeObserver(place)
    observer.observe(container)
    if (barRef.current) {
      observer.observe(barRef.current)
    }
    return () => observer.disconnect()
  }, [ containerRef, anchorIndex ])

  // Drawn unseen until it is placed, so its first placing already knows how wide it is.
  const style: CSSProperties = position
    ? { top: position.top, left: position.left ?? undefined, right: position.right ?? undefined }
    : { top: 0, left: 0, visibility: 'hidden' }

  return <div
    ref={barRef}
    role='toolbar'
    aria-label={t('review.bar.label')}
    className={[
      'absolute z-20 flex w-max max-w-full flex-wrap items-center gap-1 rounded-xl border border-border',
      'bg-overlay p-1 shadow-lg',
      position?.isBelow
        ? ''
        : '-translate-y-full',
    ].join(' ')}
    style={style}
    // A press on the bar is not a press on the words under it.
    onPointerDown={(event) => event.stopPropagation()}
    onClick={(event) => event.stopPropagation()}
  >
    {Boolean(props.pills.length) && <ToggleButtonGroup
      aria-label={t('review.bar.marks')}
      selectionMode='single'
      disallowEmptySelection
      size='sm'
      selectedKeys={props.selectedMarkId
        ? [ props.selectedMarkId ]
        : []}
      onSelectionChange={(keys) => {
        const [ markId ] = [ ...keys ]
        if (typeof markId === 'string') {
          props.onSelectMark(markId)
        }
      }}
    >{
        props.pills.map((pill, index) => <ToggleButton key={pill.markId} id={pill.markId}>
          {Boolean(index) && <ToggleButtonGroup.Separator />}
          <span>{pill.label}</span>
        </ToggleButton>)
      }</ToggleButtonGroup>}

    {props.onSelectFromHere && !props.hasSelection && <Button
      size='sm'
      variant='ghost'
      onPress={props.onSelectFromHere}
    >
      <LuTextSelect className='size-4' aria-hidden />
      <span>{t('review.bar.selectFromHere')}</span>
    </Button>}

    {props.isExtending && <span className='px-2 text-xs opacity-70'>{t('review.bar.tapToEnd')}</span>}

    {props.onPlayFromHere && <Button size='sm' variant='ghost' onPress={props.onPlayFromHere}>
      <LuPlay className='size-4' aria-hidden />
      <span>{t('review.bar.playFromHere')}</span>
      <Kbd className='ml-1 hidden sm:inline-flex' aria-hidden>
        <Kbd.Content>{t('review.bar.spaceKey')}</Kbd.Content>
      </Kbd>
    </Button>}

    {props.hasSelection && <>
      <Button size='sm' variant='ghost' onPress={props.onEdit}>
        <LuPencil className='size-4' aria-hidden />
        <span>{t('review.bar.edit')}</span>
        <Kbd className='ml-1 hidden sm:inline-flex' aria-hidden>
          <Kbd.Content>E</Kbd.Content>
        </Kbd>
      </Button>
      <Button size='sm' variant='ghost' onPress={props.onComment}>
        <LuMessageSquarePlus className='size-4' aria-hidden />
        <span>{t('review.bar.comment')}</span>
        <Kbd className='ml-1 hidden sm:inline-flex' aria-hidden>
          <Kbd.Content>C</Kbd.Content>
        </Kbd>
      </Button>
      <Tooltip delay={300}>
        <Button
          isIconOnly
          size='sm'
          variant='ghost'
          aria-label={t('review.bar.clear')}
          onPress={props.onClear}
        >
          <LuX className='size-4' aria-hidden />
        </Button>
        <Tooltip.Content>{t('review.bar.clearHint')}</Tooltip.Content>
      </Tooltip>
    </>}
  </div>
}

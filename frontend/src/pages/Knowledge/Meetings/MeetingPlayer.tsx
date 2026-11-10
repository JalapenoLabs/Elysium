// Copyright © 2026 Jalapeno Labs

import type { CSSProperties, PointerEvent as ReactPointerEvent } from 'react'
import type { MeetingCapture } from '../../../api/routes/meetingRoutes'
import type { MeetingPlayerState } from './useMeetingPlayer'

// Core
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { LuGripHorizontal } from 'react-icons/lu'
import { OptionSelect } from '../../../components/OptionSelect'
import { PlayerWaveform } from './PlayerWaveform'

// Misc
import { pixelsOf, placementAt } from './meetingPlayer'
import { usePlayerPlacement } from './usePlayerPlacement'

type Props = {
  player: MeetingPlayerState
}

// The window's size and the floating player's, which where it may go depends on.
type Room = {
  viewportWidth: number
  viewportHeight: number
  width: number
  height: number
}

type Drag = {
  pointerId: number
  // Where in the player the pointer took hold of it.
  offsetX: number
  offsetY: number
}

// What the switcher calls each capture: its source, numbered when a meeting has two from one.
function captureLabels(captures: MeetingCapture[], nameOf: (capture: MeetingCapture) => string) {
  const totals = new Map<string, number>()
  for (const capture of captures) {
    totals.set(capture.sourceKind, (totals.get(capture.sourceKind) ?? 0) + 1)
  }
  const seen = new Map<string, number>()
  return captures.map((capture) => {
    const number = (seen.get(capture.sourceKind) ?? 0) + 1
    seen.set(capture.sourceKind, number)
    return {
      id: capture.id,
      label: (totals.get(capture.sourceKind) ?? 0) > 1
        ? `${nameOf(capture)} ${number}`
        : nameOf(capture),
    }
  })
}

// A meeting's player along the top of its page (docs/meeting-review.md, The player). Once the page
// scrolls past it, it floats in a corner, where it can be dragged anywhere and stays where it was
// left, for this person on this browser. It is one media element throughout, moved rather than
// made again, so it plays on as it goes. A recording with audio alone plays over black with its
// waveform, and a meeting with several recordings has a switcher between them.
export function MeetingPlayer(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors' ])
  const { player } = props
  const slotRef = useRef<HTMLDivElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const drag = useRef<Drag | null>(null)
  const placement = usePlayerPlacement()
  const [ isFloating, setIsFloating ] = useState(false)
  const [ room, setRoom ] = useState<Room | null>(null)
  // The height the player takes along the top, which its place keeps while it floats, so the page
  // does not jump as it leaves.
  const [ inFlowHeight, setInFlowHeight ] = useState<number | null>(null)

  // It floats once its place has scrolled up out of sight, and comes back as its place returns. Out
  // of sight is out of its scroller's view, which starts below the topbar, so a place that is not
  // seen and starts above the window's bottom has gone up past it.
  useEffect(() => {
    const slot = slotRef.current
    if (!slot) {
      return undefined
    }
    const observer = new IntersectionObserver(([ entry ]) => {
      const bottom = entry.rootBounds?.bottom ?? window.innerHeight
      setIsFloating(!entry.isIntersecting && entry.boundingClientRect.top < bottom)
    })
    observer.observe(slot)
    return () => observer.disconnect()
  }, [])

  useLayoutEffect(() => {
    const box = boxRef.current
    if (!box) {
      return undefined
    }
    const measure = () => {
      if (isFloating) {
        setRoom({
          viewportWidth: window.innerWidth,
          viewportHeight: window.innerHeight,
          width: box.offsetWidth,
          height: box.offsetHeight,
        })
        return
      }
      setInFlowHeight(box.offsetHeight)
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(box)
    window.addEventListener('resize', measure)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', measure)
    }
  }, [ isFloating ])

  function startDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const box = boxRef.current
    if (!box || event.button !== 0) {
      return
    }
    const rect = box.getBoundingClientRect()
    drag.current = {
      pointerId: event.pointerId,
      offsetX: event.clientX - rect.left,
      offsetY: event.clientY - rect.top,
    }
    event.currentTarget.setPointerCapture(event.pointerId)
    event.preventDefault()
  }

  function placeAt(event: ReactPointerEvent<HTMLDivElement>) {
    const current = drag.current
    if (!current || current.pointerId !== event.pointerId || !room) {
      return null
    }
    return placementAt(event.clientX - current.offsetX, event.clientY - current.offsetY, room)
  }

  function moveDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const next = placeAt(event)
    if (next) {
      placement.move(next)
    }
  }

  function endDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const next = placeAt(event)
    drag.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
    if (next) {
      placement.save(next)
    }
  }

  if (!player.capture || !player.src) {
    return null
  }

  let floatingStyle: CSSProperties | undefined
  if (isFloating && room) {
    floatingStyle = pixelsOf(placement.placement, room)
  }
  else if (isFloating) {
    // Its first frame afloat, before it is measured: the corner it starts in.
    floatingStyle = { right: 16, bottom: 16 }
  }

  const options = captureLabels(
    player.playable,
    (capture) => t(`connectors:kinds.${capture.sourceKind}.name`),
  )

  return <div
    ref={slotRef}
    className='relaxed w-full max-w-2xl'
    style={isFloating && inFlowHeight !== null
      ? { height: inFlowHeight }
      : undefined}
  >
    <div
      ref={boxRef}
      role='region'
      aria-label={t('review.player.label')}
      className={isFloating
        ? 'fixed z-30 w-72 rounded-xl border border-border bg-overlay p-1.5 shadow-2xl sm:w-80'
        : 'w-full'}
      style={floatingStyle}
    >
      {isFloating && <div
        title={t('review.player.move')}
        className='flex cursor-grab touch-none items-center justify-center pb-1 opacity-60
          active:cursor-grabbing'
        onPointerDown={startDrag}
        onPointerMove={moveDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <LuGripHorizontal className='size-4' aria-hidden />
      </div>}

      <div className={[
        'relative overflow-hidden rounded-lg bg-black',
        player.isAudioOnly
          ? 'h-32'
          : 'aspect-video',
      ].join(' ')}>
        {/* The recording's own controls: a scrubber, volume, and full screen in every browser. */}
        <video
          ref={player.mediaRef}
          src={player.src}
          preload='metadata'
          controls
          playsInline
          aria-label={t('review.player.label')}
          className='absolute inset-0 size-full'
          {...player.mediaHandlers}
        />
        {player.isAudioOnly && !player.hasFailed && <PlayerWaveform
          mediaRef={player.mediaRef}
          isPlaying={player.isPlaying}
        />}
        {player.hasFailed && <p className='pointer-events-none absolute inset-0 grid place-items-center
          px-4 text-center text-sm text-white/70'>{
            t('review.player.failed')
          }</p>}
      </div>

      {options.length > 1 && <div className={isFloating
        ? 'mt-1.5'
        : 'mt-2 max-w-xs'}>
        <OptionSelect
          id='meeting-player-capture'
          label={t('review.player.capture')}
          isLabelHidden={isFloating}
          options={options}
          value={player.capture.id}
          onChange={player.switchCapture}
        />
      </div>}
    </div>
  </div>
}

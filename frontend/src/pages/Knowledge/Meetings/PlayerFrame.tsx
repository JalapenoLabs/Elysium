// Copyright © 2026 Jalapeno Labs

import type { CSSProperties } from 'react'
import type { MeetingCapture } from '../../../api/routes/meetingRoutes'
import type { Room } from './meetingPlayer'
import type { MeetingPlayerState } from './useMeetingPlayer'

// Core
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { LuGripHorizontal } from 'react-icons/lu'
import { OptionSelect } from '../../../components/OptionSelect'
import { PlayerWaveform } from './PlayerWaveform'

// Misc
import { meetingSourceNameKey } from '../../Settings/Meetings/meetingsPresentation'
import { PLAYER_EDGE_PX, pixelsOf } from './meetingPlayer'
import { usePlayerPlacement } from './usePlayerPlacement'

type Props = {
  player: MeetingPlayerState
  // What it plays: the capture chosen, and its recording's address.
  capture: MeetingCapture
  src: string
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

// The player itself, along the top of the page (docs/meeting-review.md, The player), mounted only
// while there is a recording to play, so the place it watches and the box it measures are always
// there as it starts watching them. Once the page scrolls past its place, it floats in a corner,
// where its grip drags it anywhere and it stays where it was left (`usePlayerPlacement`). It is one
// media element throughout, moved rather than made again, so it plays on as it goes, through a
// switch of capture too. A recording with audio alone plays over black with its waveform.
export function PlayerFrame(props: Props) {
  const { t } = useTranslation([ 'meetings', 'connectors' ])
  const { player } = props
  const slotRef = useRef<HTMLDivElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const [ isFloating, setIsFloating ] = useState(false)
  const [ room, setRoom ] = useState<Room | null>(null)
  const placement = usePlayerPlacement(boxRef, room)
  // The height the player takes along the top, which its place keeps while it floats, so the page
  // does not jump as it leaves.
  const [ inFlowHeight, setInFlowHeight ] = useState<number | null>(null)

  // It floats once its place has scrolled up out of sight, and comes back as its place returns. Out
  // of sight is out of its scroller's view, which starts below the topbar, so a place that is not
  // seen and starts above the window's bottom has gone up past it.
  useEffect(() => {
    const slot = slotRef.current
    if (!slot) {
      console.debug('The meeting player has no place to watch')
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
      console.debug('The meeting player has no box to measure')
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

  let floatingStyle: CSSProperties | undefined
  if (isFloating && room) {
    floatingStyle = pixelsOf(placement.placement, room)
  }
  else if (isFloating) {
    // Its first frame afloat, before it is measured: the corner it starts in.
    floatingStyle = { right: PLAYER_EDGE_PX, bottom: PLAYER_EDGE_PX }
  }

  const options = captureLabels(
    player.playable,
    (capture) => t(meetingSourceNameKey(capture.sourceKind)),
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
      // Where Space and the arrows play, as on the transcript (`useReviewHotkeys`). It takes focus
      // from a press on its controls, never from Tab.
      data-player-keys=''
      tabIndex={-1}
      className={isFloating
        ? 'fixed z-30 w-72 rounded-xl border border-border bg-overlay p-1.5 shadow-2xl outline-none sm:w-80'
        : 'w-full outline-none'}
      style={floatingStyle}
    >
      {isFloating && <div
        title={t('review.player.move')}
        className='flex cursor-grab touch-none items-center justify-center pb-1 opacity-60
          active:cursor-grabbing'
        {...placement.gripHandlers}
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
          ref={player.attachMedia}
          src={props.src}
          preload='metadata'
          controls
          playsInline
          aria-label={t('review.player.label')}
          className='absolute inset-0 size-full'
          {...player.mediaHandlers}
          // Focused, a browser's own controls keep every key, and every press, from the page, so
          // the arrows would stop jumping. A mouse that focuses them hands focus to the player
          // around them, where Space and the arrows play (`useReviewHotkeys`); a keyboard that tabs
          // into them, which shows its focus, keeps them.
          onFocus={(event) => {
            if (!event.currentTarget.matches(':focus-visible')) {
              boxRef.current?.focus({ preventScroll: true })
            }
          }}
        />
        {player.isAudioOnly && !player.hasFailed && <PlayerWaveform
          analyser={player.analyser}
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
          value={props.capture.id}
          onChange={player.switchCapture}
        />
      </div>}
    </div>
  </div>
}

// Copyright © 2026 Jalapeno Labs

import type { SyntheticEvent } from 'react'
import type { MeetingCapture, MeetingWord } from '../../../api/routes/meetingRoutes'

// Core
import { useCallback, useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'

// Misc
import { getCaptureMediaUrl } from '../../../api/routes/meetingRoutes'
import {
  captureSecondsAt,
  isVideo,
  meetingMsAt,
  playableCapturesOf,
  spokenWordAt,
} from './meetingPlayer'

type Options = {
  meetingId: string
  captures: MeetingCapture[]
  words: MeetingWord[]
}

// What a switch of capture carries to the next recording, which can seek only once it has loaded:
// the moment of the meeting it was at, and whether it was playing.
type Carried = {
  meetingMs: number
  resume: boolean
}

// What the recording that loaded turned out to be, for the capture it belongs to.
type Loaded = {
  captureId: string
  isAudioOnly: boolean
  hasFailed: boolean
}

// A meeting's player (docs/meeting-review.md, The player): one media element, playing one capture
// at a time, which the page seeks from the words and its keys. Everything it is asked for is on
// the meeting's clock, and moved onto the capture's own by the capture's offset, so a switch of
// capture plays on from the same moment. While it plays, the word being spoken is read every
// frame, and the page redraws only when that word changes.
export function useMeetingPlayer(options: Options) {
  const { playable, preferred } = useMemo(() => playableCapturesOf(options.captures), [ options.captures ])
  const [ chosenId, setChosenId ] = useState<string | null>(null)
  // A capture chosen and since gone, such as one split back out of the meeting, gives way.
  const capture = playable.find((candidate) => candidate.id === chosenId) ?? preferred
  const mediaRef = useRef<HTMLVideoElement | null>(null)
  const carried = useRef<Carried | null>(null)
  const [ isPlaying, setIsPlaying ] = useState(false)
  const [ spokenIndex, setSpokenIndex ] = useState<number | null>(null)
  const [ loaded, setLoaded ] = useState<Loaded | null>(null)

  const offsetMs = capture?.offsetMs ?? 0
  const isLoaded = Boolean(capture) && loaded?.captureId === capture?.id
  // Until its recording says, a capture is what its type says it is.
  const isAudioOnly = isLoaded
    ? Boolean(loaded?.isAudioOnly)
    : !isVideo(capture?.recordingContentType ?? null)

  // Nothing is spoken on a page just opened; once the player has played or sought, its position
  // is the word being spoken, paused or not.
  const hasMoved = useRef(false)

  const readSpoken = useEffectEvent(() => {
    const media = mediaRef.current
    if (!media || !hasMoved.current) {
      return
    }
    // The same word again changes nothing, so React draws nothing.
    setSpokenIndex(spokenWordAt(options.words, meetingMsAt(media.currentTime, offsetMs)))
  })

  // An edit renumbers the words after it, so the word being spoken is found again in them.
  useEffect(() => {
    readSpoken()
  }, [ options.words ])

  // `timeupdate` comes four times a second, too seldom for words a quarter second long.
  useEffect(() => {
    if (!isPlaying) {
      return undefined
    }
    let frame = 0
    const tick = () => {
      readSpoken()
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [ isPlaying ])

  const play = useCallback(() => {
    const media = mediaRef.current
    if (!media) {
      console.debug('The meeting player was asked to play with no recording to play')
      return
    }
    media.play().catch((error: unknown) => {
      // A browser refuses to play without a gesture, or the recording failed to load; the
      // player shows the second, and the person presses play for the first.
      console.debug('The meeting player could not start playing', { error })
    })
  }, [])

  const seek = useCallback((meetingMs: number) => {
    const media = mediaRef.current
    if (!media || !capture) {
      console.debug('The meeting player was asked to seek with no recording to seek', { meetingMs })
      return
    }
    // Before its metadata loads, this is where it starts once it does.
    media.currentTime = captureSecondsAt(meetingMs, capture.offsetMs)
  }, [ capture ])

  const toggle = useCallback(() => {
    const media = mediaRef.current
    if (media && !media.paused) {
      media.pause()
      return
    }
    play()
  }, [ play ])

  const playFrom = useCallback((meetingMs: number) => {
    seek(meetingMs)
    play()
  }, [ seek, play ])

  const jump = useCallback((deltaMs: number) => {
    const media = mediaRef.current
    if (!media) {
      console.debug('The meeting player was asked to jump with no recording to jump in', { deltaMs })
      return
    }
    seek(meetingMsAt(media.currentTime, offsetMs) + deltaMs)
  }, [ seek, offsetMs ])

  const switchCapture = useCallback((captureId: string) => {
    const media = mediaRef.current
    if (media) {
      carried.current = {
        meetingMs: meetingMsAt(media.currentTime, offsetMs),
        resume: !media.paused,
      }
    }
    setChosenId(captureId)
  }, [ offsetMs ])

  const mediaHandlers = {
    onPlay: () => {
      hasMoved.current = true
      setIsPlaying(true)
    },
    onPause: () => setIsPlaying(false),
    onEnded: () => setIsPlaying(false),
    // A seek while paused, such as a word clicked, moves the spoken word too.
    onSeeked: (event: SyntheticEvent<HTMLVideoElement>) => {
      hasMoved.current = true
      const meetingMs = meetingMsAt(event.currentTarget.currentTime, offsetMs)
      setSpokenIndex(spokenWordAt(options.words, meetingMs))
    },
    onLoadedMetadata: (event: SyntheticEvent<HTMLVideoElement>) => {
      const media = event.currentTarget
      if (!capture) {
        return
      }
      setLoaded({ captureId: capture.id, isAudioOnly: media.videoWidth === 0, hasFailed: false })
      const moment = carried.current
      carried.current = null
      if (!moment) {
        return
      }
      media.currentTime = captureSecondsAt(moment.meetingMs, capture.offsetMs)
      if (moment.resume) {
        play()
      }
    },
    onError: (event: SyntheticEvent<HTMLVideoElement>) => {
      console.debug('The meeting player could not load a recording', {
        captureId: capture?.id,
        error: event.currentTarget.error,
      })
      if (capture) {
        setLoaded({ captureId: capture.id, isAudioOnly, hasFailed: true })
      }
      setIsPlaying(false)
    },
  }

  return {
    capture,
    playable,
    src: capture
      ? getCaptureMediaUrl(options.meetingId, capture.id)
      : null,
    mediaRef,
    mediaHandlers,
    isPlaying,
    isAudioOnly,
    hasFailed: isLoaded && Boolean(loaded?.hasFailed),
    spokenIndex,
    play,
    seek,
    toggle,
    playFrom,
    jump,
    switchCapture,
  } as const
}

export type MeetingPlayerState = ReturnType<typeof useMeetingPlayer>

// Copyright © 2026 Jalapeno Labs

import type { MeetingTranscriptChange } from '../../../api/routes/meetingRoutes'

// Core
import { useCallback } from 'react'
import useSWR from 'swr'

// Misc
import { getMeetingWords, meetingWordsKey } from '../../../api/routes/meetingRoutes'
import { applyTranscriptChange } from './meetingTranscript'

// A meeting's final transcript for its page. No other view shows one, so it is read through SWR
// alone (docs/frontend.md, Server data), and `meeting.changed` revalidates it. A change the page
// made applies at once from what its route answered (`applyChange`), with no read of the whole
// transcript; the event's read that follows brings the same words.
export function useMeetingWords(meetingId: string) {
  const { data, error, mutate } = useSWR(
    meetingWordsKey(meetingId),
    () => getMeetingWords(meetingId),
    { keepPreviousData: true },
  )

  const applyChange = useCallback((change: MeetingTranscriptChange) => {
    void mutate(
      (current) => current && applyTranscriptChange(current, change),
      { revalidate: false },
    )
  }, [ mutate ])

  return {
    transcript: data,
    hasFailed: Boolean(error) && !data,
    applyChange,
    reload: () => void mutate(),
  } as const
}

export type MeetingWordsState = ReturnType<typeof useMeetingWords>

// Copyright © 2026 Jalapeno Labs

// Core
import { useMemo } from 'react'
import useSWR from 'swr'

// Misc
import { getMeetingHistory, meetingHistoryKey } from '../../../api/routes/meetingRoutes'
import { describeHistory } from './meetingReviewPresentation'

// A meeting's history, newest first, for its page: one read through SWR, which a change the page
// makes and `meeting.changed` revalidate.
export function useMeetingHistory(meetingId: string) {
  const { data, error } = useSWR(
    meetingHistoryKey(meetingId),
    () => getMeetingHistory(meetingId),
    { keepPreviousData: true },
  )
  const rows = useMemo(() => describeHistory(data?.entries ?? [], meetingId), [ data, meetingId ])

  return {
    entries: data?.entries ?? null,
    rows,
    hasFailed: Boolean(error) && !data,
  } as const
}

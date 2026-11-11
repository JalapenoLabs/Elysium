// Copyright © 2026 Jalapeno Labs

// Core
import useSWR from 'swr'

// Misc
import { getMeetingThreads, meetingThreadsKey } from '../../../api/routes/meetingRoutes'

// What is written in a meeting's comment threads, for its page: one read through SWR, since no
// other view shows it, which a change the page makes and `meeting.changed` revalidate. Where each
// thread lies, and whether it is resolved, is its mark's, read with the transcript.
export function useMeetingThreads(meetingId: string) {
  const { data, error } = useSWR(
    meetingThreadsKey(meetingId),
    () => getMeetingThreads(meetingId),
    { keepPreviousData: true },
  )

  return {
    comments: data?.comments ?? null,
    hasFailed: Boolean(error) && !data,
  } as const
}

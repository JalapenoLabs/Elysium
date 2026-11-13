// Copyright © 2026 Jalapeno Labs

// Core
import useSWR from 'swr'

// Misc
import { getMeetingTasks, meetingTasksKey } from '../../../api/routes/meetingRoutes'

// The tasks heard in a meeting, for the bar along its page's top: one read through SWR, since no
// other view shows them, which `meeting.changed` revalidates, and so does a changed changeset or
// item, which may be one a task was made into.
export function useMeetingTasks(meetingId: string) {
  const { data } = useSWR(
    meetingTasksKey(meetingId),
    () => getMeetingTasks(meetingId),
    {
      keepPreviousData: true,
      onError: (error: unknown) => console.debug('useMeetingTasks could not read the meeting\'s tasks', {
        error,
        meetingId,
      }),
    },
  )

  return data?.tasks ?? null
}

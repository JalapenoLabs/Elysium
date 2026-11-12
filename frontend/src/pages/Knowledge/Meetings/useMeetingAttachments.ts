// Copyright © 2026 Jalapeno Labs

// Core
import useSWR from 'swr'

// Misc
import { getMeetingAttachmentPolicy, MEETING_ATTACHMENT_POLICY_KEY } from '../../../api/routes/meetingAttachmentRoutes'
import { listMeetingAttachments, meetingAttachmentsKey } from '../../../api/routes/meetingRoutes'

// A meeting's files and frames, for its page: one read through SWR, since no other view shows them,
// which a change the page makes and `meeting.changed` revalidate. Where each is pinned is its pins',
// read with the transcript's marks.
export function useMeetingAttachments(meetingId: string) {
  const { data, error } = useSWR(
    meetingAttachmentsKey(meetingId),
    () => listMeetingAttachments(meetingId),
    { keepPreviousData: true },
  )

  return {
    attachments: data?.attachments ?? null,
    hasFailed: Boolean(error) && !data,
  } as const
}

// What a meeting takes as a file or a frame, and where they go. `meetingAttachments.changed` and
// `pageAttachments.changed` load it again, so an admin's choice reaches every meeting's page.
export function useMeetingAttachmentPolicy() {
  const { data, error } = useSWR(MEETING_ATTACHMENT_POLICY_KEY, getMeetingAttachmentPolicy)

  return {
    policy: data?.policy ?? null,
    hasFailed: Boolean(error),
  } as const
}

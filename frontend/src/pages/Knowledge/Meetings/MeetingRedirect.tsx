// Copyright © 2026 Jalapeno Labs

// Core
import { Navigate, useLocation, useParams } from 'react-router'

// Misc
import { getMeetingUrl } from '../../../urls'

// `/settings/meetings/:meetingId`, the address links and bookmarks to a meeting held before its
// page moved into the knowledge base, where it lands with its query, so `?tab=topics` still opens
// its topics.
export function MeetingRedirect() {
  const { meetingId = '' } = useParams()
  const { search } = useLocation()

  return <Navigate to={`${getMeetingUrl(meetingId)}${search}`} replace />
}

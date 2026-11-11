// Copyright © 2026 Jalapeno Labs

// The changes a meeting's page made itself and already applied from what its route answered, so
// the `meeting.changed` each one announces is not read as news and the transcript, tens of
// thousands of words, is not read again for it (docs/meetings.md, In the web app). Another tab
// never registered the change, so it reads it as anyone's.
//
// The announcement can arrive before the route's answer, so a change is tracked from the moment
// it is sent: an event for that meeting waits for the changes in flight to settle before asking.

const inFlight = new Map<string, Set<Promise<unknown>>>()
const applied = new Set<string>()

// Tracks one change to meeting `meetingId` while it is sent; once it answers with its history
// entry, that entry is the page's own.
export async function trackMeetingChange<Answer extends { entry: { id: string } }>(
  meetingId: string,
  request: Promise<Answer>,
) {
  const pending = inFlight.get(meetingId) ?? new Set<Promise<unknown>>()
  inFlight.set(meetingId, pending)
  pending.add(request)
  try {
    const answer = await request
    applied.add(answer.entry.id)
    return answer
  }
  finally {
    pending.delete(request)
    if (!pending.size) {
      inFlight.delete(meetingId)
    }
  }
}

// Whether entry `entryId` of meeting `meetingId` is a change this page made and applied. Each is
// answered once, so a later event naming it again reads as news.
export async function isOwnMeetingChange(meetingId: string, entryId: string) {
  await Promise.allSettled([ ...inFlight.get(meetingId) ?? [] ])
  return applied.delete(entryId)
}

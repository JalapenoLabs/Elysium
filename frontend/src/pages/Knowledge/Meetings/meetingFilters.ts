// Copyright © 2026 Jalapeno Labs

import type { SmartTableFilterValues } from '@jalapenolabs/uikit'
import type { ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'
import type { Meeting, MeetingState } from '../../../api/routes/meetingRoutes'

// Misc
import { MEETING_STATES, UPLOAD_SOURCE_KIND } from '../../../api/routes/meetingRoutes'
import { MEETINGS_PROJECT_PARAM } from '../../../urls'
import { NO_PROJECT } from '../../ActionItems/actionItemFilters'

// The Meetings page's filters, kept in the address as the item list keeps its own, edited in the
// table's filters drawer and shown as its tags. Elysium applies them itself (`filterMeetings`), so
// the page knows when nothing matches. A project's Meetings group in the tree opens the page with
// its project chosen.

export type MeetingFilters = {
  // The kinds of source they came from, as `Meeting.sourceKind` names them: a connector kind, or a
  // source that is none, such as an upload. Empty shows every source.
  sources: string[]
  // How far each one's processing got. Empty shows every state.
  states: MeetingState[]
  // A project its topics are on, `NO_PROJECT` for a meeting on none, or null for any.
  project: string | null
  // Only meetings whose run waits for the person's review of the corrected transcript.
  waitingForReview: boolean
}

// Each filter's id in the drawer, which is also its parameter in the address.
export const MEETING_FILTER_IDS = {
  sources: 'source',
  states: 'state',
  project: MEETINGS_PROJECT_PARAM,
  waitingForReview: 'review',
} as const satisfies Record<keyof MeetingFilters, string>

// A source is any kind a meeting names, connector or not, so the address keeps whatever it held; one
// no meeting came from matches nothing, and the page says so.
function readSources(values: string[]) {
  return values.filter(Boolean)
}

function readStates(values: string[]) {
  return MEETING_STATES.filter((state) => values.includes(state))
}

export function readMeetingFilters(params: URLSearchParams): MeetingFilters {
  return {
    sources: readSources(params.get(MEETING_FILTER_IDS.sources)?.split(',') ?? []),
    states: readStates(params.get(MEETING_FILTER_IDS.states)?.split(',') ?? []),
    project: params.get(MEETING_FILTER_IDS.project),
    waitingForReview: params.get(MEETING_FILTER_IDS.waitingForReview) === 'true',
  }
}

// Only what narrows goes in the address, so every meeting has the plain one.
export function writeMeetingFilters(filters: MeetingFilters) {
  const params = new URLSearchParams()
  if (filters.sources.length) {
    params.set(MEETING_FILTER_IDS.sources, filters.sources.join(','))
  }
  if (filters.states.length) {
    params.set(MEETING_FILTER_IDS.states, filters.states.join(','))
  }
  if (filters.project) {
    params.set(MEETING_FILTER_IDS.project, filters.project)
  }
  if (filters.waitingForReview) {
    params.set(MEETING_FILTER_IDS.waitingForReview, 'true')
  }
  return params
}

// The filters as the drawer and its tags show them.
export function toMeetingFilterValues(filters: MeetingFilters): SmartTableFilterValues {
  return {
    [MEETING_FILTER_IDS.sources]: filters.sources.length
      ? filters.sources
      : null,
    [MEETING_FILTER_IDS.states]: filters.states.length
      ? filters.states
      : null,
    [MEETING_FILTER_IDS.project]: filters.project,
    [MEETING_FILTER_IDS.waitingForReview]: filters.waitingForReview || null,
  }
}

// The filters the drawer applied, or a tag removed.
export function fromMeetingFilterValues(values: SmartTableFilterValues): MeetingFilters {
  const sourceValue = values[MEETING_FILTER_IDS.sources]
  const stateValue = values[MEETING_FILTER_IDS.states]
  const projectValue = values[MEETING_FILTER_IDS.project]

  return {
    sources: Array.isArray(sourceValue)
      ? readSources(sourceValue)
      : [],
    states: Array.isArray(stateValue)
      ? readStates(stateValue)
      : [],
    project: typeof projectValue === 'string' && projectValue
      ? projectValue
      : null,
    waitingForReview: values[MEETING_FILTER_IDS.waitingForReview] === true,
  }
}

// The sources the filter offers: every kind whose definition provides meetings, in the hub's order,
// then uploads, which anyone can make, so a source is offered before it has read anything; then any
// other source a meeting came from, in the order the meetings first name it.
export function listMeetingSourceKinds(definitions: ConnectorKindDefinition[], meetings: Meeting[]) {
  const kinds = new Set<string>()
  for (const definition of definitions) {
    if (definition.provides.includes('meetings')) {
      kinds.add(definition.kind)
    }
  }
  kinds.add(UPLOAD_SOURCE_KIND)
  for (const meeting of meetings) {
    kinds.add(meeting.sourceKind)
  }
  return [ ...kinds ]
}

// The meetings that pass every filter, in the order given, and how many of all of them wait for
// review, which the page counts whatever the filters leave, in one pass.
export function filterMeetings(meetings: Meeting[], filters: MeetingFilters) {
  const matches: Meeting[] = []
  let waitingCount = 0

  for (const meeting of meetings) {
    if (meeting.awaitingApproval) {
      waitingCount += 1
    }
    if (filters.sources.length && !filters.sources.includes(meeting.sourceKind)) {
      continue
    }
    if (filters.states.length && !filters.states.includes(meeting.state)) {
      continue
    }
    if (filters.project === NO_PROJECT && meeting.projectIds.length) {
      continue
    }
    if (filters.project && filters.project !== NO_PROJECT && !meeting.projectIds.includes(filters.project)) {
      continue
    }
    if (filters.waitingForReview && !meeting.awaitingApproval) {
      continue
    }
    matches.push(meeting)
  }

  return { matches, waitingCount } as const
}

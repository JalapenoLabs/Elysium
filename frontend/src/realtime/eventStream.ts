// Copyright © 2026 Jalapeno Labs

import type { ServerEvent, ServerEventType } from './serverEvents'

// Core
import { mutate } from 'swr'

// Redux
import { store } from '../store'
import { codingSessionDeleted, codingSessionUpserted } from '../store/codingSessionsSlice'
import { llmDeleted, llmUpserted } from '../store/llmsSlice'
import { mailAccountDeleted, mailAccountUpserted } from '../store/mailAccountsSlice'
import { mailDomainDeleted, mailDomainUpserted } from '../store/mailDomainsSlice'
import { mailServerUpdated } from '../store/mailServerSlice'
import { projectDeleted, projectUpserted } from '../store/projectsSlice'
import { eventStreamLost, eventStreamOpened } from '../store/realtimeSlice'
import { satelliteDeleted, satelliteStatusReported, satelliteUpserted } from '../store/satellitesSlice'
import { sessionEventReceived } from '../store/sessionEventsSlice'

// Misc
import { EVENT_STREAM_PATH, EVENT_STREAM_RETRY_INITIAL_MS, EVENT_STREAM_RETRY_MAX_MS } from '../constants'

// The universal event stream: one EventSource for the whole app, open for as long as
// the page is. Every change the API announces lands in Redux through here, so no
// component polls or refetches after another tab's write.

type Handlers = {
  [Type in ServerEventType]: (event: Extract<ServerEvent, { type: Type }>) => void
}

// Refetches everything on screen: every SWR key a mounted component holds, whose
// loaders put the responses in Redux (`src/hooks/useServerData.ts`). Runs on every
// (re)connection, because events sent while disconnected are gone for good. Data no
// component shows is refetched when one mounts.
function reloadEverything() {
  void mutate(() => true)
}

const handlers: Handlers = {
  'hello': () => reloadEverything(),
  'resync': () => reloadEverything(),
  'llm.upserted': (event) => store.dispatch(llmUpserted(event.data)),
  'llm.deleted': (event) => store.dispatch(llmDeleted(event.data.id)),
  'mailbox.upserted': (event) => store.dispatch(mailAccountUpserted(event.data)),
  'mailbox.deleted': (event) => store.dispatch(mailAccountDeleted(event.data.id)),
  'mailServer.updated': (event) => store.dispatch(mailServerUpdated(event.data)),
  'mailDomain.upserted': (event) => store.dispatch(mailDomainUpserted(event.data)),
  'mailDomain.deleted': (event) => store.dispatch(mailDomainDeleted(event.data.id)),
  'project.upserted': (event) => store.dispatch(projectUpserted(event.data)),
  'project.deleted': (event) => store.dispatch(projectDeleted(event.data.id)),
  'satellite.upserted': (event) => store.dispatch(satelliteUpserted(event.data)),
  'satellite.deleted': (event) => store.dispatch(satelliteDeleted(event.data.id)),
  'satellite.status': (event) => store.dispatch(satelliteStatusReported(event.data)),
  'session.upserted': (event) => store.dispatch(codingSessionUpserted(event.data)),
  'session.deleted': (event) => store.dispatch(codingSessionDeleted(event.data.id)),
  'session.event': (event) => store.dispatch(sessionEventReceived(event.data)),
  // Revalidates only if a conversation panel holds this session's history key.
  'session.resync': (event) => void mutate(`v1/coding-sessions/${event.data.id}/events`),
}

function dispatchServerEvent(data: string) {
  let event: ServerEvent
  try {
    event = JSON.parse(data)
  }
  catch (error) {
    console.debug('Ignoring an event stream message that is not JSON', { error, data })
    return
  }

  // Narrowed by `type` at runtime; the lookup keeps every known type handled.
  const handler = handlers[event.type] as ((event: ServerEvent) => void) | undefined
  if (!handler) {
    console.debug('Ignoring an event of a type this build does not know', { type: event.type })
    return
  }
  handler(event)
}

// Opens the stream and keeps it open. Browsers retry a dropped EventSource by
// themselves, but close it for good after an HTTP error response, such as a 502 while
// the API restarts; this reopens it with a backoff in that case.
export function startEventStream() {
  let retryDelay = EVENT_STREAM_RETRY_INITIAL_MS

  function open() {
    const source = new EventSource(EVENT_STREAM_PATH)

    source.addEventListener('open', () => {
      retryDelay = EVENT_STREAM_RETRY_INITIAL_MS
      store.dispatch(eventStreamOpened())
    })

    source.addEventListener('message', (message) => dispatchServerEvent(message.data))

    source.addEventListener('error', () => {
      store.dispatch(eventStreamLost())
      if (source.readyState !== EventSource.CLOSED) {
        return
      }

      console.debug('Event stream closed by an error response; reopening', { retryDelay })
      window.setTimeout(open, retryDelay)
      retryDelay = Math.min(retryDelay * 2, EVENT_STREAM_RETRY_MAX_MS)
    })
  }

  open()
}

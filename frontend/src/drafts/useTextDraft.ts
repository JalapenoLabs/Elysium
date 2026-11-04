// Copyright © 2026 Jalapeno Labs

// Core
import { useCallback, useState } from 'react'

// Misc
import { forgetSessionDraft, keepSessionDraft, readSessionDraft } from './sessionDrafts'

function readTextDraft(name: string) {
  const stored = readSessionDraft(name)
  if (stored === null || typeof stored === 'string') {
    return stored
  }
  console.debug('A text draft held something other than text, so it was left out', { name })
  return null
}

// One box of text kept as a draft for the tab, such as a prompt, a comment, or a reply, used
// like state. `text` is what was typed, or null while nothing is, so a caller can show
// something else in its place (a reply box shows the record's unsent draft). `change` keeps
// what is typed; `forget` ends the draft once it is sent or discarded. Blanking the box keeps
// the last words in storage (see `keepSessionDraft`), so they come back on a reload.
export function useTextDraft(name: string) {
  const [ draft, setDraft ] = useState(() => ({ name, text: readTextDraft(name) }))

  // A component that moves to another record's box, such as a panel reused for another
  // session, picks up that record's draft rather than carrying this one over.
  let current = draft
  if (draft.name !== name) {
    current = { name, text: readTextDraft(name) }
    setDraft(current)
  }

  const change = useCallback((text: string) => {
    setDraft({ name, text })
    keepSessionDraft(name, text, [ text ])
  }, [ name ])

  const forget = useCallback(() => {
    setDraft({ name, text: null })
    forgetSessionDraft(name)
  }, [ name ])

  return { text: current.text, change, forget } as const
}

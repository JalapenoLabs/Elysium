// Copyright © 2026 Jalapeno Labs

// Core
import { useRef, useState } from 'react'

// Which of a component's actions is running, so its button can show it working
// (`~/.claude/docs/ux.md`, Buttons) while the others wait. One runs at a time: a press while
// another is in flight is ignored, so two writes never race.
//
// Keys name the action, and the record it acts on where a list repeats it (a session's id,
// `<operation id>:<decision>`). A failure is passed on to the caller, since the action is what
// knows how to report it.
export function usePendingAction<Key extends string>() {
  const [ pendingKey, setPendingKey ] = useState<Key | null>(null)
  // Read by `run`, so two presses in one render cannot both start before state updates.
  const runningKey = useRef<Key | null>(null)

  async function run(key: Key, action: () => Promise<unknown>) {
    if (runningKey.current !== null) {
      console.debug('usePendingAction ignored an action while another runs', {
        key,
        running: runningKey.current,
      })
      return
    }
    runningKey.current = key
    setPendingKey(key)
    try {
      await action()
    }
    finally {
      runningKey.current = null
      setPendingKey(null)
    }
  }

  // Whether an action other than this one runs, for its button's `isDisabled`. The running
  // one is pending rather than disabled, so it shows working instead of greyed out.
  function isOtherPending(key: Key) {
    return pendingKey !== null && pendingKey !== key
  }

  return {
    pendingKey,
    isAnyPending: pendingKey !== null,
    isOtherPending,
    run,
  } as const
}

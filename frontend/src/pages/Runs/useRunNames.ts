// Copyright © 2026 Jalapeno Labs

import type { RunNameKind } from './runPresentation'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Misc
import { nameFromIdentifier, runNameKeys } from './runPresentation'

// What the Runs pages call a workflow, a step, or a source kind: its words from the
// translations, or its name in code as words when the web app has none for it yet.
// Stable while the language is, so a table's memoized columns can depend on it.
export function useRunNames() {
  const { t } = useTranslation('runs')

  return useMemo(() => {
    function nameOf(kind: RunNameKind, identifier: string) {
      const key = runNameKeys[kind].get(identifier)
      if (!key) {
        return nameFromIdentifier(identifier)
      }
      return t(key)
    }

    return {
      workflow: (workflow: string) => nameOf('workflows', workflow),
      step: (step: string) => nameOf('steps', step),
      source: (sourceKind: string) => nameOf('sources', sourceKind),
    } as const
  }, [ t ])
}

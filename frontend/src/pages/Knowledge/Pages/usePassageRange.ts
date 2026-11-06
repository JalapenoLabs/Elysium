// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { useMemo } from 'react'
import useSWRImmutable from 'swr/immutable'

// Misc
import { getPagePassage, pagePassageKey } from '../../../api/routes/pageRoutes'
import { withoutTitle } from '../knowledgePresentation'
import { findPassageRange } from './pagePassage'

// Where the passage at `position` lies in `page`'s Markdown, or null while that is not known and
// when it is nowhere: a link older than the text, or evidence, which is not Markdown of the page.
// The page then simply opens at its top. The index cut the passage, so it is read back from the
// index rather than cut again here, and keyed by version, since publishing can recut the text.
export function usePassageRange(page: KnowledgePage | undefined, position: number | null) {
  const key = page && position !== null
    ? pagePassageKey(page.id, position, page.currentVersion)
    : null
  const { data } = useSWRImmutable(
    key,
    ([ , pageId, passagePosition ]) => getPagePassage(pageId, passagePosition),
    {
      // A newer version's read keeps the mark rather than dropping it for a moment.
      keepPreviousData: true,
      shouldRetryOnError: false,
      onError: (error) => console.debug('usePassageRange could not read a passage', { key, error }),
    },
  )

  const body = page?.body
  const title = page?.title
  const passage = data?.passage
  return useMemo(() => {
    if (body === undefined || title === undefined || !passage) {
      return null
    }
    return findPassageRange(body, withoutTitle(passage, title))
  }, [ body, title, passage ])
}

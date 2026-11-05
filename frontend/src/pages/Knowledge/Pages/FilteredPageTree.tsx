// Copyright © 2026 Jalapeno Labs

import type { Key } from 'react-aria-components'
import type { PageTree as PageTreeShape, PageTreeFilter } from './pageTree'
import type { PageTreeMemoryState } from './usePageTreeMemory'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { PageTree } from './PageTree'

// Misc
import { filterPageTree } from './pageTree'

type Props = {
  tree: PageTreeShape
  filter: PageTreeFilter
  memory: PageTreeMemoryState
  onNavigate?: () => void
}

// The tree as its filter leaves it. A filter's matches are the point, so every branch holding one
// shows open, and closing one lasts as long as that filter does: the branches a person keeps open
// stay as they left them for when the filter comes off.
export function FilteredPageTree(props: Props) {
  const { t } = useTranslation('knowledge')
  const filtered = useMemo(() => filterPageTree(props.tree, props.filter), [ props.tree, props.filter ])
  const [ closed, setClosed ] = useState({ filter: props.filter, ids: new Set<Key>() })

  if (filtered === props.tree) {
    return <PageTree
      tree={props.tree}
      openIds={props.memory.openIds}
      onOpenIdsChange={props.memory.setOpenIds}
      onOpenBranches={props.memory.openBranches}
      onNavigate={props.onNavigate}
    />
  }

  if (!filtered.byId.size) {
    return <p role='status' className='px-2.5 text-sm opacity-70'>{t('labels.unmatched')}</p>
  }

  const branchIds = [ ...filtered.childrenByParent.keys() ].filter((id) => id !== null)
  const openIds = new Set(branchIds)
  // Branches closed under an earlier filter say nothing about this one.
  if (closed.filter === props.filter) {
    for (const id of closed.ids) {
      openIds.delete(String(id))
    }
  }

  return <PageTree
    tree={props.tree}
    shownTree={filtered}
    openIds={openIds}
    onOpenIdsChange={(ids) => setClosed({
      filter: props.filter,
      ids: new Set(branchIds.filter((id) => !ids.has(id))),
    })}
    onOpenBranches={props.memory.openBranches}
    onNavigate={props.onNavigate}
  />
}

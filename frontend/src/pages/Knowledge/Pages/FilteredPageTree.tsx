// Copyright © 2026 Jalapeno Labs

import type { Key } from 'react-aria-components'
import type { PageTree as PageTreeShape, PageTreeFilter } from './pageTree'
import type { PageTreeMemoryState } from './usePageTreeMemory'

// Core
import { memo, useDeferredValue, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { PageTree } from './PageTree'

// Misc
import { getKnowledgePagesSearchUrl } from '../../../urls'
import { filterPageTree, holdsSamePages } from './pageTree'

type Props = {
  tree: PageTreeShape
  filter: PageTreeFilter
  memory: PageTreeMemoryState
  onNavigate?: () => void
}

type ShownTreeProps = Pick<PageTreeMemoryState, 'openIds' | 'setOpenIds' | 'openBranches'> & {
  tree: PageTreeShape
  filtered: PageTreeShape
  onNavigate?: () => void
}

// Memoized, so typing passes it by until a filter changes what shows: drawing a large tree outlasts
// several keystrokes. Every branch holding a match shows open, and closing one lasts as long as
// those matches do, while the branches a person keeps open stay as they left them.
const ShownPageTree = memo(function ShownPageTree(props: ShownTreeProps) {
  const { filtered } = props
  const [ closed, setClosed ] = useState({ filtered, ids: new Set<Key>() })

  if (filtered === props.tree) {
    return <PageTree
      tree={props.tree}
      openIds={props.openIds}
      onOpenIdsChange={props.setOpenIds}
      onOpenBranches={props.openBranches}
      onNavigate={props.onNavigate}
    />
  }

  const branchIds = [ ...filtered.childrenByParent.keys() ].filter((id) => id !== null)
  const openIds = new Set(branchIds)
  // Branches closed under earlier matches say nothing about these.
  if (closed.filtered === filtered) {
    for (const id of closed.ids) {
      openIds.delete(String(id))
    }
  }

  return <PageTree
    tree={props.tree}
    shownTree={filtered}
    openIds={openIds}
    onOpenIdsChange={(ids) => setClosed({
      filtered,
      ids: new Set(branchIds.filter((id) => !ids.has(id))),
    })}
    onOpenBranches={props.openBranches}
    onNavigate={props.onNavigate}
  />
})

// The tree as its filter leaves it. The filter is deferred, so the field keeps up with typing and a
// tree drawn for a letter already replaced is abandoned, and a letter that leaves the same pages
// showing, as most do once a word has narrowed them, draws nothing at all.
export function FilteredPageTree(props: Props) {
  const { t } = useTranslation('knowledge')
  const filter = useDeferredValue(props.filter)
  const latest = useMemo(() => filterPageTree(props.tree, filter), [ props.tree, filter ])
  const [ drawn, setDrawn ] = useState(latest)
  const filtered = holdsSamePages(drawn, latest)
    ? drawn
    : latest
  if (filtered !== drawn) {
    setDrawn(filtered)
  }

  if (!filtered.byId.size) {
    return <div role='status' className='px-2.5 text-sm'>
      <p className='opacity-70'>{
        filter.title
          ? t('pages.unmatched')
          : t('labels.unmatched')
      }</p>
      {filter.title && <Link
        href={getKnowledgePagesSearchUrl(filter.title)}
        className='text-link'
        onPress={props.onNavigate}
      >{
          t('pages.searchText')
        }</Link>}
    </div>
  }

  return <ShownPageTree
    tree={props.tree}
    filtered={filtered}
    openIds={props.memory.openIds}
    setOpenIds={props.memory.setOpenIds}
    openBranches={props.memory.openBranches}
    onNavigate={props.onNavigate}
  />
}

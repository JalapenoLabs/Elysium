// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInboxActionItems, selectNextActionItems } from '../../store/actionItemsSlice'
import { selectPendingChangesets } from '../../store/changesetsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { ChangesetsLeadCard } from '../Changesets/ChangesetsLeadCard'
import { InboxLeadCard } from './InboxLeadCard'
import { NextEmptyState } from './NextEmptyState'
import { NextItemCard } from './NextItemCard'

// Misc
import { useActionItemsLoader } from '../../hooks/useServerData'
import { useNow } from '../../hooks/useNow'
import { pickCurrentItem, skipItem } from './nextRotation'

// `/action-items`, the default view: one item at a time from Next, with its context and
// quick actions. Acting on an item takes it out of Next, so the next one takes its place at
// once. Skipping sets an item aside for this visit only. While the inbox holds anything, a
// card leading the page offers to triage it.
export function NextPage() {
  const { t } = useTranslation('actionItems')
  const status = useActionItemsLoader()
  const now = useNow()
  const nextItems = useAppSelector((state) => selectNextActionItems(state, now))
  const inboxCount = useAppSelector(selectInboxActionItems).length
  const pendingChangesetCount = useAppSelector(selectPendingChangesets).length
  const [ skippedIds, setSkippedIds ] = useState<string[]>([])

  if (status === 'loading') {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }
  if (status === 'failed') {
    return <p className='py-10 text-center text-sm text-danger'>{t('next.loadError')}</p>
  }

  const current = pickCurrentItem(nextItems, skippedIds)

  return <div>
    {pendingChangesetCount > 0 && <ChangesetsLeadCard count={pendingChangesetCount} />}
    {inboxCount > 0 && <InboxLeadCard count={inboxCount} />}
    {current
      ? <NextItemCard
        key={current.id}
        item={current}
        position={nextItems.indexOf(current) + 1}
        total={nextItems.length}
        now={now}
        onSkip={() => setSkippedIds((skipped) => skipItem(nextItems, skipped, current.id))}
      />
      : <NextEmptyState now={now} />}
  </div>
}

// Copyright © 2026 Jalapeno Labs

import type { Changeset } from '../../api/routes/changesetRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectChangesets } from '../../store/changesetsSlice'

// User interface
import { Spinner, Tabs } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { PageTabs } from '../../components/PageTabs'
import { ChangesetListItem } from './ChangesetListItem'

// Misc
import { useChangesetsLoader } from '../../hooks/useServerData'
import { useTabParam } from '../../hooks/useTabParam'

const TABS = [ 'pending', 'reviewed' ] as const

// `/action-items/changesets`: the changesets waiting for review, and on a tab of their own the
// ones already decided, each newest first. The open tab lives in the address.
export function ChangesetsPage() {
  const { t } = useTranslation('changesets')
  const status = useChangesetsLoader()
  const changesets = useAppSelector(selectChangesets)
  const [ selectedTab, selectTab ] = useTabParam(TABS)

  if (status === 'loading') {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }
  if (status === 'failed' && !changesets.length) {
    return <p className='text-sm text-danger'>{t('list.loadError')}</p>
  }

  const pending: Changeset[] = []
  const reviewed: Changeset[] = []
  for (const changeset of changesets) {
    if (changeset.state === 'pending') {
      pending.push(changeset)
    }
    else {
      reviewed.push(changeset)
    }
  }

  return <PageTabs
    label={t('tabs.label')}
    selectedKey={selectedTab}
    onSelectionChange={selectTab}
    tabs={[
      { id: 'pending', label: t('tabs.pending'), count: pending.length, isWaiting: true },
      { id: 'reviewed', label: t('tabs.reviewed'), count: reviewed.length },
    ]}
  >
    <Tabs.Panel id='pending'>{
      pending.length
        ? pending.map((changeset) => <ChangesetListItem key={changeset.id} changeset={changeset} />)
        : <EmptyNotice>
          <span className='block font-semibold'>{t('list.emptyTitle')}</span>
          <span>{t('list.emptyBody')}</span>
        </EmptyNotice>
    }</Tabs.Panel>
    <Tabs.Panel id='reviewed'>{
      reviewed.length
        ? reviewed.map((changeset) => <ChangesetListItem key={changeset.id} changeset={changeset} />)
        : <EmptyNotice>{t('list.noneReviewed')}</EmptyNotice>
    }</Tabs.Panel>
  </PageTabs>
}

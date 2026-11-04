// Copyright © 2026 Jalapeno Labs

import type { Changeset } from '../../api/routes/changesetRoutes'
import type { ChangesetList } from './ChangesetTable'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectChangesets } from '../../store/changesetsSlice'

// User interface
import { Spinner, Tabs } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { PageTabs } from '../../components/PageTabs'
import { ChangesetTable } from './ChangesetTable'

// Misc
import { useChangesetsLoader } from '../../hooks/useServerData'
import { useTabParam } from '../../hooks/useTabParam'
import { usePageToolsContainer } from '../../layout/page/pageTools'

const TABS = [ 'pending', 'reviewed' ] as const satisfies readonly ChangesetList[]

// `/action-items/changesets`: the changesets waiting for review, and on a tab of their own the
// ones already decided, each a table newest first, its search, Columns, and export in the Action
// items titlebar. Only the open tab's panel is mounted, so one table at a time holds the
// titlebar. The open tab lives in the address.
export function ChangesetsPage() {
  const { t } = useTranslation('changesets')
  const toolsContainer = usePageToolsContainer()
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
    level='nested'
    selectedKey={selectedTab}
    onSelectionChange={selectTab}
    tabs={[
      { id: 'pending', label: t('tabs.pending'), count: pending.length, isWaiting: true },
      { id: 'reviewed', label: t('tabs.reviewed'), count: reviewed.length },
    ]}
  >
    <Tabs.Panel id='pending'>
      <ChangesetTable
        changesets={pending}
        list='pending'
        controlsContainer={toolsContainer}
        emptyNotice={<EmptyNotice>
          <span className='block font-semibold'>{t('list.emptyTitle')}</span>
          <span>{t('list.emptyBody')}</span>
        </EmptyNotice>}
      />
    </Tabs.Panel>
    <Tabs.Panel id='reviewed'>
      <ChangesetTable
        changesets={reviewed}
        list='reviewed'
        controlsContainer={toolsContainer}
        emptyNotice={<EmptyNotice>{t('list.noneReviewed')}</EmptyNotice>}
      />
    </Tabs.Panel>
  </PageTabs>
}

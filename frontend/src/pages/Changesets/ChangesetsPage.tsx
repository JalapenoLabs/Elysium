// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectChangesets } from '../../store/changesetsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ChangesetListItem } from './ChangesetListItem'

// Misc
import { useChangesetsLoader } from '../../hooks/useServerData'

// `/action-items/changesets`: every changeset, the ones waiting for review first, then the
// ones already decided, each newest first.
export function ChangesetsPage() {
  const { t } = useTranslation('changesets')
  const status = useChangesetsLoader()
  const changesets = useAppSelector(selectChangesets)

  if (status === 'loading') {
    return <div className='grid place-items-center py-16'>
      <Spinner />
    </div>
  }
  if (status === 'failed' && !changesets.length) {
    return <p className='text-sm text-danger'>{t('list.loadError')}</p>
  }
  if (!changesets.length) {
    return <EmptyNotice>
      <span className='block font-semibold'>{t('list.emptyTitle')}</span>
      <span>{t('list.emptyBody')}</span>
    </EmptyNotice>
  }

  const pending = []
  const reviewed = []
  for (const changeset of changesets) {
    if (changeset.state === 'pending') {
      pending.push(changeset)
    }
    else {
      reviewed.push(changeset)
    }
  }

  return <div>
    {pending.length > 0 && <section className='relaxed'>
      <h2 className='compact text-lg font-semibold'>{t('list.pendingHeading')}</h2>
      {pending.map((changeset) => <ChangesetListItem key={changeset.id} changeset={changeset} />)}
    </section>}
    {reviewed.length > 0 && <section className='relaxed'>
      <h2 className='compact text-lg font-semibold'>{t('list.reviewedHeading')}</h2>
      {reviewed.map((changeset) => <ChangesetListItem key={changeset.id} changeset={changeset} />)}
    </section>}
  </div>
}

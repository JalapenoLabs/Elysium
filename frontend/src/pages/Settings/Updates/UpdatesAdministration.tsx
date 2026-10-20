// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectUpdatesStatus } from '../../../store/updatesSlice'

// User interface
import { Spinner } from '@heroui/react'
import { UpdateHistoryTable } from './UpdateHistoryTable'
import { UpdatesOverview } from './UpdatesOverview'

// Misc
import { useUpdatesLoader } from '../../../hooks/useServerData'

// Loads auto-update's status once; the event stream keeps it current from then on.
export function UpdatesAdministration() {
  const { t } = useTranslation('updates')
  const loadStatus = useUpdatesLoader()
  const status = useAppSelector(selectUpdatesStatus)

  if (!status) {
    return loadStatus === 'failed'
      ? <p className='py-10 text-center text-sm text-danger'>{t('loadError')}</p>
      : <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
  }

  return <>
    <p className='relaxed max-w-2xl text-sm opacity-70'>{t('description')}</p>
    <UpdatesOverview status={status} />
    <section>
      <h2 className='compact text-xl font-semibold'>{t('history.heading')}</h2>
      <UpdateHistoryTable updates={status.history} />
    </section>
  </>
}

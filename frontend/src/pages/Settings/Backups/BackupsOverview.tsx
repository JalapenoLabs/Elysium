// Copyright © 2026 Jalapeno Labs

import type { BackupsStatus } from '../../../api/routes/backupRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { formatDateTime } from '../../../format/dates'
import { formatBytes } from '../../../format/numbers'
import { summarizeHistory } from './backupsPresentation'

type Props = {
  status: BackupsStatus
}

// Where backups go, the last backup stored, when the next one runs, and how long backups are
// kept.
export function BackupsOverview(props: Props) {
  const { t } = useTranslation('backups')
  const status = props.status
  const { lastStored, isRunning } = summarizeHistory(status.history)

  let lastStoredText = t('overview.nothingStored')
  if (lastStored) {
    lastStoredText = t('overview.storedAt', {
      storedAt: formatDateTime(lastStored.finishedAt ?? lastStored.startedAt),
      size: formatBytes(lastStored.sizeBytes ?? 0),
    })
  }

  let nextText = t('overview.off')
  if (isRunning) {
    nextText = t('overview.running')
  }
  else if (status.nextAttemptAt) {
    // A night that is due already starts at the schedule's next check, within minutes.
    const nextAttemptAt = new Date(status.nextAttemptAt)
    nextText = nextAttemptAt.getTime() <= Date.now()
      ? t('overview.soon')
      : formatDateTime(nextAttemptAt)
  }

  return <section className='relaxed grid gap-6 rounded-xl border border-separator p-6 sm:grid-cols-2 lg:grid-cols-4'>
    <div>
      <p className='text-sm opacity-70'>{t('overview.destination')}</p>
      <p className={status.destination
        ? 'font-semibold'
        : 'font-semibold text-warning'}>{
          status.destination?.name ?? t('destination.none')
        }</p>
    </div>
    <div>
      <p className='text-sm opacity-70'>{t('overview.lastStored')}</p>
      <p className='font-semibold'>{lastStoredText}</p>
    </div>
    <div>
      <p className='text-sm opacity-70'>{t('overview.next')}</p>
      <p className={status.destination
        ? 'font-semibold'
        : 'font-semibold text-warning'}>{nextText}</p>
    </div>
    <div>
      <p className='text-sm opacity-70'>{t('overview.kept')}</p>
      <p className='font-semibold'>{t('overview.keptDays', { count: status.retentionDays })}</p>
    </div>
  </section>
}

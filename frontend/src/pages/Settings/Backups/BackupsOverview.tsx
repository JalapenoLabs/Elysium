// Copyright © 2026 Jalapeno Labs

import type { BackupsStatus } from '../../../api/routes/backupRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { formatStorageBytes } from '../Storage/storagePresentation'
import { summarizeHistory } from './backupsPresentation'

type Props = {
  status: BackupsStatus
}

const dateTimeFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' })

// The last backup stored, when the next one runs, and how long backups are kept.
export function BackupsOverview(props: Props) {
  const { t, i18n } = useTranslation('backups')
  const status = props.status
  const { lastStored, isRunning } = summarizeHistory(status.history)

  let lastStoredText = t('overview.nothingStored')
  if (lastStored) {
    lastStoredText = t('overview.storedAt', {
      storedAt: dateTimeFormatter.format(new Date(lastStored.finishedAt ?? lastStored.startedAt)),
      size: formatStorageBytes(lastStored.sizeBytes ?? 0, i18n.language),
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
      : dateTimeFormatter.format(nextAttemptAt)
  }

  return <section className='relaxed grid gap-6 rounded-xl border border-separator p-6 sm:grid-cols-3'>
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

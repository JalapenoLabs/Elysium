// Copyright © 2026 Jalapeno Labs

import type { BatchConfiguration } from '../../api/routes/agentConfigurationRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { ConfigurationSkippedList } from '../../components/ConfigurationSkippedList'

// Misc
import { commitUrl, shortCommit } from '../../components/agentConfigurationPresentation'
import { formatDateTime } from '../../format/dates'

type Props = {
  configuration: BatchConfiguration
}

// The configuration repository commit a batch's worker runs on, and what its container left out
// of it, as its worker recorded them before its first turn.
export function WorkBatchConfiguration(props: Props) {
  const { t } = useTranslation('workQueue')
  const configuration = props.configuration
  const link = commitUrl(configuration.repository, configuration.commit)

  return <section className='relaxed max-w-3xl'>
    <h2 className='compact text-lg font-semibold'>{t('batch.configuration.title')}</h2>
    <p className='compact text-sm'>
      {link
        ? <Link href={link} target='_blank' rel='noreferrer' className='font-mono text-link'>{
          shortCommit(configuration.commit)
        }</Link>
        : <span className='font-mono'>{shortCommit(configuration.commit)}</span>}
      <span className='ml-2'>{configuration.subject}</span>
    </p>
    <p className='relaxed text-sm opacity-70'>{
      t('batch.configuration.fetched', {
        repository: configuration.repository,
        time: formatDateTime(configuration.fetchedAt),
      })
    }</p>
    <h3 className='compact font-semibold'>{t('batch.configuration.skipped')}</h3>
    <ConfigurationSkippedList skipped={configuration.skipped} />
  </section>
}

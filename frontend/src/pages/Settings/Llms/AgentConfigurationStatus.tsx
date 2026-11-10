// Copyright © 2026 Jalapeno Labs

import type { WorkspaceSettings } from '../../../api/routes/userRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Link, Tooltip } from '@heroui/react'
import { LuRefreshCw } from 'react-icons/lu'
import { ConfigurationHoldAlert } from '../../../components/ConfigurationHoldAlert'
import { ConfigurationSkippedList } from '../../../components/ConfigurationSkippedList'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { fetchAgentConfiguration } from '../../../api/routes/agentConfigurationRoutes'
import { commitUrl, shortCommit } from '../../../components/agentConfigurationPresentation'
import { formatDateTime, formatRelativeTime } from '../../../format/dates'
import { useNow } from '../../../hooks/useNow'
import { toast } from '../../../toast/toast'

type Props = {
  settings: WorkspaceSettings
  // The page's repository or token differ from what is saved. A fetch reads the saved ones, so
  // Fetch now waits for Save rather than fetching what the page no longer shows.
  hasUnsavedChanges: boolean
}

// How the saved repository's last fetch went: why workers and Project Managers wait, if they do,
// the commit fetched and when, and what it left out. The event stream brings each fetch's
// outcome, so this follows a save or a Fetch now without reloading.
export function AgentConfigurationStatus(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])
  const now = useNow()
  const [ isAsking, setIsAsking ] = useState(false)
  const repository = props.settings.agentConfigurationRepository
  const { hold, checkout } = props.settings.agentConfiguration

  async function fetchNow() {
    setIsAsking(true)
    try {
      await fetchAgentConfiguration()
      toast.success(t('agentConfiguration.fetchAsked'))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('AgentConfigurationStatus could not ask for a fetch', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
    }
    finally {
      setIsAsking(false)
    }
  }

  const link = checkout && repository
    ? commitUrl(repository, checkout.commit)
    : null

  return <section className='relaxed max-w-2xl'>
    <div className='compact flex flex-wrap items-center justify-between gap-3'>
      <h2 className='text-lg font-semibold'>{t('agentConfiguration.lastFetch')}</h2>
      {repository && <Tooltip delay={200} isDisabled={!props.hasUnsavedChanges}>
        <Tooltip.Trigger>
          <div>
            <Button
              variant='secondary'
              size='sm'
              isDisabled={props.hasUnsavedChanges}
              isPending={isAsking}
              onPress={() => void fetchNow()}
            >
              <LuRefreshCw className='size-4' aria-hidden />
              <span>{t('agentConfiguration.fetchNow')}</span>
            </Button>
          </div>
        </Tooltip.Trigger>
        <Tooltip.Content>{t('agentConfiguration.fetchNeedsSave')}</Tooltip.Content>
      </Tooltip>}
    </div>

    {hold && <ConfigurationHoldAlert hold={hold} linksToSettings={false} />}

    {checkout && <div className='relaxed'>
      <p className='compact text-sm'>
        {link
          ? <Link href={link} target='_blank' rel='noreferrer' className='font-mono text-link'>{
            shortCommit(checkout.commit)
          }</Link>
          : <span className='font-mono'>{shortCommit(checkout.commit)}</span>}
        <span className='ml-2'>{checkout.subject}</span>
      </p>
      <Tooltip delay={300}>
        <Tooltip.Trigger>
          <span className='text-sm opacity-70'>{
            t('agentConfiguration.fetchedAt', { time: formatRelativeTime(checkout.fetchedAt, now) })
          }</span>
        </Tooltip.Trigger>
        <Tooltip.Content>{formatDateTime(checkout.fetchedAt)}</Tooltip.Content>
      </Tooltip>
    </div>}

    {checkout && <div>
      <h3 className='compact font-semibold'>{t('agentConfiguration.skippedTitle')}</h3>
      <ConfigurationSkippedList skipped={checkout.skipped} />
    </div>}
  </section>
}

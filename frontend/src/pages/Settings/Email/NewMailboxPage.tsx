// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectAllMailDomains } from '../../../store/mailDomainsSlice'
import { selectMailServer } from '../../../store/mailServerSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { CreateMailboxForm } from './CreateMailboxForm'

// Misc
import { useMailDomainsLoader, useMailServerLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'
import { getMailboxBlockerKey } from './mailPresentation'

// `/settings/email/mailboxes/new`: creating a mailbox on the mail server. The form opens once
// the server and its domains are loaded, since it preselects the default domain as it mounts.
// A server that cannot take a mailbox yet is named instead, as Email's Create mailbox does.
export function NewMailboxPage() {
  const { t } = useTranslation('email')
  const serverStatus = useMailServerLoader()
  const domainsStatus = useMailDomainsLoader()
  const server = useAppSelector(selectMailServer)
  const domains = useAppSelector(selectAllMailDomains)

  const isLoading = !server || domainsStatus === 'loading'
  const hasFailed = serverStatus === 'failed' || domainsStatus === 'failed'
  const blockerKey = isLoading
    ? null
    : getMailboxBlockerKey(server, domains.length > 0)

  return <SettingsPage
    title={t('createForm.title')}
    parents={[{ label: t('title'), href: UrlTree.settingsEmail }]}
    help={t('createForm.description')}
  >
    {isLoading && !hasFailed && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {isLoading && hasFailed && <p className='py-10 text-center text-sm text-danger'>{
      t('server.loadError')
    }</p>}
    {blockerKey && <EmptyNotice>{t(blockerKey)}</EmptyNotice>}
    {!isLoading && !blockerKey && <CreateMailboxForm domains={domains} />}
  </SettingsPage>
}

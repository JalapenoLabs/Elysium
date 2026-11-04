// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectMailServer } from '../../../store/mailServerSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { SettingsPage } from '../SettingsPage'
import { MailDomainForm } from './MailDomainForm'

// Misc
import { useMailServerLoader } from '../../../hooks/useServerData'
import { getTabUrl, UrlTree } from '../../../urls'
import { mailServerBlockerKeys } from './mailPresentation'

// `/settings/email/domains/new`: adding a domain to the mail server. A server that cannot take
// one yet, because it does not exist or is not answering, is named instead of the form.
export function NewMailDomainPage() {
  const { t } = useTranslation('email')
  const status = useMailServerLoader()
  const server = useAppSelector(selectMailServer)
  const blockerKey = server
    ? mailServerBlockerKeys[server.state]
    : null

  return <SettingsPage
    title={t('domainForm.title')}
    parents={[{ label: t('title'), href: getTabUrl(UrlTree.settingsEmail, 'domains') }]}
    help={t('domainForm.description')}
  >
    {!server && status !== 'failed' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {!server && status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('server.loadError')
    }</p>}
    {blockerKey && <EmptyNotice>{t(blockerKey)}</EmptyNotice>}
    {server && !blockerKey && <MailDomainForm />}
  </SettingsPage>
}

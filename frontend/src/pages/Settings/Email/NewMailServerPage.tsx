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
import { MailServerForm } from './MailServerForm'

// Misc
import { useMailServerLoader } from '../../../hooks/useServerData'
import { getTabUrl, UrlTree } from '../../../urls'

// `/settings/email/server/new`: creating the one mail server, or trying again after it failed.
// Once one exists or is being created there is nothing to make here, and the page says so. It
// never redirects on its own: creating the server changes its state too, while the form's save,
// which goes back to Email itself, is still running.
export function NewMailServerPage() {
  const { t } = useTranslation('email')
  const status = useMailServerLoader()
  const server = useAppSelector(selectMailServer)
  const isCreatable = server?.state === 'not-created' || server?.state === 'failed'

  return <SettingsPage
    title={t('serverForm.title')}
    parents={[{ label: t('title'), href: getTabUrl(UrlTree.settingsEmail, 'domains') }]}
    help={t('serverForm.description')}
  >
    {!server && status !== 'failed' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {!server && status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('server.loadError')
    }</p>}
    {server && !isCreatable && <EmptyNotice>{t('serverForm.exists')}</EmptyNotice>}
    {isCreatable && <MailServerForm />}
  </SettingsPage>
}

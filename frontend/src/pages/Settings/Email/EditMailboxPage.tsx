// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate, useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectMailAccountById } from '../../../store/mailAccountsSlice'

// User interface
import { Link, Spinner } from '@heroui/react'
import { SettingsPage } from '../SettingsPage'
import { MailboxForm } from './MailboxForm'

// Misc
import { useMailAccountsLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/settings/email/:accountId/edit`: one mailbox's sender name and whether it is active.
export function EditMailboxPage() {
  const { t } = useTranslation('email')
  const navigate = useNavigate()
  const { accountId = '' } = useParams()
  const status = useMailAccountsLoader()
  const account = useAppSelector((state) => selectMailAccountById(state, accountId))

  if (!account) {
    if (status === 'loading') {
      return <div className='grid place-items-center py-16'>
        <Spinner />
      </div>
    }
    return <div className='container py-10 text-center text-sm'>
      <p className='compact opacity-70'>{t('mailboxForm.notFound')}</p>
      <Link href={UrlTree.settingsEmail} className='text-link'>{t('mailboxForm.backToList')}</Link>
    </div>
  }

  return <SettingsPage
    title={t('mailboxForm.title', { address: account.address })}
    parents={[{ label: t('title'), href: UrlTree.settingsEmail }]}
  >
    <MailboxForm
      // Remount when moving between mailboxes, so nothing typed for one carries to another.
      key={account.id}
      account={account}
      onSaved={() => navigate(UrlTree.settingsEmail)}
    />
  </SettingsPage>
}

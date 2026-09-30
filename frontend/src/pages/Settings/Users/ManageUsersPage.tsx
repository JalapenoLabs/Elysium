// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { UsersAdministration } from './UsersAdministration'

// Misc
import { UrlTree } from '../../../urls'

// Users: approving sign-ups and managing people's accounts. Admins only; anyone else who
// opens the address is told so, and the settings directory hides the entry from them.
export function ManageUsersPage() {
  const { t } = useTranslation([ 'users', 'settings' ])
  const isAdmin = useAppSelector(selectIsAdmin)

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('title')
    }</h1>
    {isAdmin
      ? <UsersAdministration />
      : <p className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'>{
        t('adminOnly')
      }</p>}
  </div>
}

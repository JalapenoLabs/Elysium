// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Card, Link } from '@heroui/react'
import { LuHardDrive } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// What Studio shows while no storage location exists: every file an item makes needs one.
export function StudioNoStorage() {
  const { t } = useTranslation('studio')

  return <Card className='mx-auto max-w-lg items-center p-8 text-center'>
    <LuHardDrive className='compact size-8 opacity-60' aria-hidden />
    <h2 className='compact text-lg font-semibold'>{t('noStorage.title')}</h2>
    <p className='relaxed text-sm opacity-70'>{t('noStorage.body')}</p>
    <Link href={UrlTree.settingsStorage} className='text-link'>{
      t('noStorage.action')
    }</Link>
  </Card>
}

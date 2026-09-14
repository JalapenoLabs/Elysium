// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { ThemeSelector } from './ThemeSelector'

// Misc
import { UrlTree } from '../../../urls'

export function PersonalDetailsPage() {
  const { t } = useTranslation('settings')

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('personalDetails.title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('personalDetails.title')
    }</h1>

    <section>
      <h2 className='text-xl font-semibold'>{
        t('personalDetails.appearance.heading')
      }</h2>
      <p className='relaxed mt-1 max-w-2xl text-sm opacity-70'>{
        t('personalDetails.appearance.description')
      }</p>
      <ThemeSelector />
    </section>
  </div>
}

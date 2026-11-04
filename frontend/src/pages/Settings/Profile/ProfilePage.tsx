// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SettingsPage } from '../SettingsPage'
import { ThemeSelector } from './ThemeSelector'

export function PersonalDetailsPage() {
  const { t } = useTranslation('settings')

  return <SettingsPage title={t('personalDetails.title')}>

    <section>
      <h2 className='text-xl font-semibold'>{
        t('personalDetails.appearance.heading')
      }</h2>
      <p className='relaxed mt-1 max-w-2xl text-sm opacity-70'>{
        t('personalDetails.appearance.description')
      }</p>
      <ThemeSelector />
    </section>
  </SettingsPage>
}

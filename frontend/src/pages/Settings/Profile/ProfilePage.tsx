// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Spinner } from '@heroui/react'
import { SettingsPage } from '../SettingsPage'
import { ProfileForm } from './ProfileForm'
import { ThemeSelector } from './ThemeSelector'

// Misc
import { useKratosSettingsFlow } from '../../../hooks/useKratosSettingsFlow'
import { UrlTree } from '../../../urls'

// My profile: the person's name and email, which Kratos holds and Save sends there, and how
// Elysium looks on this browser, which applies as soon as it is picked.
export function ProfilePage() {
  const { t } = useTranslation([ 'settings', 'common' ])
  const settings = useKratosSettingsFlow(UrlTree.settingsProfile)
  const flow = settings.flow

  return <SettingsPage title={t('profile.title')}>
    <section className='relaxed'>
      <h2 className='compact text-xl font-semibold'>{
        t('profile.details.heading')
      }</h2>
      {settings.loadFailed && <div className='py-6'>
        <p className='compact text-sm text-danger'>{t('profile.details.loadError')}</p>
        <Button size='sm' variant='outline' onPress={settings.retry}>
          <span>{t('common:actions.retry')}</span>
        </Button>
      </div>}
      {!flow && !settings.loadFailed && <Spinner />}
      {/* Keyed by flow so a reloaded flow starts the fields from what Kratos now holds. */}
      {flow && <ProfileForm key={flow.id} flow={flow} onSubmit={settings.submit} />}
    </section>

    <section>
      <h2 className='text-xl font-semibold'>{
        t('profile.appearance.heading')
      }</h2>
      <p className='relaxed mt-1 max-w-2xl text-sm opacity-70'>{
        t('profile.appearance.description')
      }</p>
      <ThemeSelector />
    </section>
  </SettingsPage>
}

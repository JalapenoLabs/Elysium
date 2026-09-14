// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { LuBot, LuSatellite, LuUser } from 'react-icons/lu'
import { SettingsDirectoryItem } from './SettingsDirectoryItem'

// Misc
import { UrlTree } from '../../urls'

// The directory behind the topbar's settings gear, modeled on Stripe's: grouped
// sections, each a responsive grid of entries.
export function SettingsDirectoryPage() {
  const { t } = useTranslation('settings')

  return <div className='container'>
    <section className='relaxed pt-2'>
      <h2 className='relaxed text-base font-semibold'>{
        t('sections.personal')
      }</h2>
      <div className='grid grid-cols-1 gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-3'>
        <SettingsDirectoryItem
          icon={LuUser}
          title={t('items.personalDetails.title')}
          description={t('items.personalDetails.description')}
          href={UrlTree.settingsPersonalDetails}
        />
      </div>
    </section>
    <section className='relaxed pt-6'>
      <h2 className='relaxed text-base font-semibold'>{
        t('sections.workspace')
      }</h2>
      <div className='grid grid-cols-1 gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-3'>
        <SettingsDirectoryItem
          icon={LuBot}
          title={t('items.llms.title')}
          description={t('items.llms.description')}
          href={UrlTree.settingsLlms}
        />
        <SettingsDirectoryItem
          icon={LuSatellite}
          title={t('items.satellites.title')}
          description={t('items.satellites.description')}
          href={UrlTree.settingsSatellites}
        />
      </div>
    </section>
  </div>
}

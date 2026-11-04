// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import {
  LuAppWindow,
  LuBot,
  LuDatabaseBackup,
  LuHardDrive,
  LuKeyRound,
  LuMail,
  LuPlug,
  LuRefreshCw,
  LuShieldCheck,
  LuUser,
  LuVariable,
  LuVideo,
} from 'react-icons/lu'
import { SettingsDirectoryItem } from './SettingsDirectoryItem'
import { UsersDirectoryItem } from './UsersDirectoryItem'

// Misc
import { UrlTree } from '../../urls'

// The directory behind the topbar's settings gear, modeled on Stripe's: grouped
// sections, each a responsive grid of entries.
export function SettingsDirectoryPage() {
  const { t } = useTranslation('settings')
  const isAdmin = useAppSelector(selectIsAdmin)

  return <div className='container'>
    <section className='relaxed pt-2'>
      <h2 className='relaxed text-base font-semibold'>{
        t('sections.personal')
      }</h2>
      <div className='grid grid-cols-1 gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-3'>
        <SettingsDirectoryItem
          icon={LuUser}
          title={t('items.profile.title')}
          description={t('items.profile.description')}
          href={UrlTree.settingsProfile}
        />
        <SettingsDirectoryItem
          icon={LuShieldCheck}
          title={t('items.security.title')}
          description={t('items.security.description')}
          href={UrlTree.settingsSecurity}
        />
        <SettingsDirectoryItem
          icon={LuAppWindow}
          title={t('items.browsers.title')}
          description={t('items.browsers.description')}
          href={UrlTree.settingsBrowsers}
        />
        <SettingsDirectoryItem
          icon={LuPlug}
          title={t('items.mcp.title')}
          description={t('items.mcp.description')}
          href={UrlTree.settingsMcp}
        />
        <SettingsDirectoryItem
          icon={LuVideo}
          title={t('items.meetings.title')}
          description={t('items.meetings.description')}
          href={UrlTree.settingsMeetings}
        />
      </div>
    </section>
    <section className='relaxed pt-6'>
      <h2 className='relaxed text-base font-semibold'>{
        t('sections.workspace')
      }</h2>
      <div className='grid grid-cols-1 gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-3'>
        {isAdmin && <UsersDirectoryItem />}
        <SettingsDirectoryItem
          icon={LuBot}
          title={t('items.llms.title')}
          description={t('items.llms.description')}
          href={UrlTree.settingsLlms}
        />
        <SettingsDirectoryItem
          icon={LuMail}
          title={t('items.email.title')}
          description={t('items.email.description')}
          href={UrlTree.settingsEmail}
        />
        <SettingsDirectoryItem
          icon={LuPlug}
          title={t('items.connectors.title')}
          description={t('items.connectors.description')}
          href={UrlTree.settingsConnectors}
        />
        <SettingsDirectoryItem
          icon={LuHardDrive}
          title={t('items.storage.title')}
          description={t('items.storage.description')}
          href={UrlTree.settingsStorage}
        />
        <SettingsDirectoryItem
          icon={LuVariable}
          title={t('items.environment.title')}
          description={t('items.environment.description')}
          href={UrlTree.settingsEnvironment}
        />
        {isAdmin && <SettingsDirectoryItem
          icon={LuKeyRound}
          title={t('items.passwords.title')}
          description={t('items.passwords.description')}
          href={UrlTree.settingsPasswords}
        />}
        {isAdmin && <SettingsDirectoryItem
          icon={LuRefreshCw}
          title={t('items.updates.title')}
          description={t('items.updates.description')}
          href={UrlTree.settingsUpdates}
        />}
        {isAdmin && <SettingsDirectoryItem
          icon={LuDatabaseBackup}
          title={t('items.backups.title')}
          description={t('items.backups.description')}
          href={UrlTree.settingsBackups}
        />}
      </div>
    </section>
  </div>
}

// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin, selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import {
  LuBookOpen,
  LuBot,
  LuCable,
  LuDatabaseBackup,
  LuHardDrive,
  LuKeyRound,
  LuLightbulb,
  LuMail,
  LuPlug,
  LuRefreshCw,
  LuShieldCheck,
  LuTags,
  LuUser,
  LuVariable,
  LuVideo,
} from 'react-icons/lu'
import { Page } from '../../layout/page/Page'
import { SettingsDirectoryItem } from './SettingsDirectoryItem'
import { UsersDirectoryItem } from './UsersDirectoryItem'

// Misc
import { UrlTree } from '../../urls'

// The directory behind the topbar's settings gear, modeled on Stripe's: the Settings title,
// then grouped sections, each a responsive grid of tiles.
export function SettingsDirectoryPage() {
  const { t } = useTranslation('settings')
  const isAdmin = useAppSelector(selectIsAdmin)
  const isCompanyMember = useAppSelector(selectIsCompanyMember)

  return <Page title={t('title')}>
    <section className='relaxed'>
      <h2 className='compact text-base font-semibold'>{
        t('sections.personal')
      }</h2>
      <div className='-mx-3 grid grid-cols-1 gap-x-4 gap-y-2 md:grid-cols-2 xl:grid-cols-3'>
        <SettingsDirectoryItem
          icon={LuUser}
          title={t('items.account.title')}
          href={UrlTree.settingsAccount}
        />
        <SettingsDirectoryItem
          icon={LuShieldCheck}
          title={t('items.security.title')}
          href={UrlTree.settingsSecurity}
        />
        <SettingsDirectoryItem
          icon={LuCable}
          title={t('items.mcp.title')}
          href={UrlTree.settingsMcp}
        />
      </div>
    </section>
    <section className='relaxed pt-6'>
      <h2 className='compact text-base font-semibold'>{
        t('sections.knowledgeBase')
      }</h2>
      <div className='-mx-3 grid grid-cols-1 gap-x-4 gap-y-2 md:grid-cols-2 xl:grid-cols-3'>
        <SettingsDirectoryItem
          icon={LuVideo}
          title={t('items.meetings.title')}
          href={UrlTree.settingsMeetings}
        />
        {isCompanyMember && <SettingsDirectoryItem
          icon={LuBookOpen}
          title={t('items.companyDoc.title')}
          href={UrlTree.settingsCompanyDoc}
        />}
        {isCompanyMember && <SettingsDirectoryItem
          icon={LuLightbulb}
          title={t('items.learnings.title')}
          href={UrlTree.settingsLearnings}
        />}
        {isAdmin && <SettingsDirectoryItem
          icon={LuTags}
          title={t('items.learningCategories.title')}
          href={UrlTree.settingsLearningCategories}
        />}
      </div>
    </section>
    <section className='relaxed pt-6'>
      <h2 className='compact text-base font-semibold'>{
        t('sections.workspace')
      }</h2>
      <div className='-mx-3 grid grid-cols-1 gap-x-4 gap-y-2 md:grid-cols-2 xl:grid-cols-3'>
        {isAdmin && <UsersDirectoryItem />}
        <SettingsDirectoryItem
          icon={LuBot}
          title={t('items.llms.title')}
          href={UrlTree.settingsLlms}
        />
        <SettingsDirectoryItem
          icon={LuMail}
          title={t('items.email.title')}
          href={UrlTree.settingsEmail}
        />
        <SettingsDirectoryItem
          icon={LuPlug}
          title={t('items.connectors.title')}
          href={UrlTree.settingsConnectors}
        />
        <SettingsDirectoryItem
          icon={LuHardDrive}
          title={t('items.storage.title')}
          href={UrlTree.settingsStorage}
        />
        <SettingsDirectoryItem
          icon={LuVariable}
          title={t('items.environment.title')}
          href={UrlTree.settingsEnvironment}
        />
        {isAdmin && <SettingsDirectoryItem
          icon={LuKeyRound}
          title={t('items.passwords.title')}
          href={UrlTree.settingsPasswords}
        />}
        {isAdmin && <SettingsDirectoryItem
          icon={LuRefreshCw}
          title={t('items.updates.title')}
          href={UrlTree.settingsUpdates}
        />}
        {isAdmin && <SettingsDirectoryItem
          icon={LuDatabaseBackup}
          title={t('items.backups.title')}
          href={UrlTree.settingsBackups}
        />}
      </div>
    </section>
  </Page>
}

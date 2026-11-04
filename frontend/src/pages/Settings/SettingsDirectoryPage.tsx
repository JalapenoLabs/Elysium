// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin, selectIsCompanyMember } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import {
  LuBot,
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
// then grouped sections, each a responsive grid of entries.
export function SettingsDirectoryPage() {
  const { t } = useTranslation('settings')
  const isAdmin = useAppSelector(selectIsAdmin)
  const isCompanyMember = useAppSelector(selectIsCompanyMember)

  return <Page title={t('title')}>
    <section className='relaxed'>
      <h2 className='relaxed text-base font-semibold'>{
        t('sections.personal')
      }</h2>
      <div className='grid grid-cols-1 gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-3'>
        <SettingsDirectoryItem
          icon={LuUser}
          title={t('items.account.title')}
          description={t('items.account.description')}
          href={UrlTree.settingsAccount}
        />
        <SettingsDirectoryItem
          icon={LuShieldCheck}
          title={t('items.security.title')}
          description={t('items.security.description')}
          href={UrlTree.settingsSecurity}
        />
        <SettingsDirectoryItem
          icon={LuPlug}
          title={t('items.mcp.title')}
          description={t('items.mcp.description')}
          href={UrlTree.settingsMcp}
        />
      </div>
    </section>
    <section className='relaxed pt-6'>
      <h2 className='relaxed text-base font-semibold'>{
        t('sections.knowledgeBase')
      }</h2>
      <div className='grid grid-cols-1 gap-x-10 gap-y-8 md:grid-cols-2 xl:grid-cols-3'>
        <SettingsDirectoryItem
          icon={LuVideo}
          title={t('items.meetings.title')}
          description={t('items.meetings.description')}
          href={UrlTree.settingsMeetings}
        />
        {isCompanyMember && <SettingsDirectoryItem
          icon={LuLightbulb}
          title={t('items.learnings.title')}
          description={t('items.learnings.description')}
          href={UrlTree.settingsLearnings}
        />}
        {isAdmin && <SettingsDirectoryItem
          icon={LuTags}
          title={t('items.learningCategories.title')}
          description={t('items.learningCategories.description')}
          href={UrlTree.settingsLearningCategories}
        />}
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
  </Page>
}

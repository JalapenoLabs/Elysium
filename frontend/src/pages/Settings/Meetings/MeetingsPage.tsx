// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { FirefliesSection } from './FirefliesSection'
import { MeetingList } from './MeetingList'

// Misc
import { UrlTree } from '../../../urls'

// `/settings/meetings`: the person's meeting sources, and their recent meetings with how far
// each one's processing got. See docs/meetings.md.
export function MeetingsPage() {
  const { t } = useTranslation([ 'meetings', 'settings' ])

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('title')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='compact text-3xl font-bold'>{t('title')}</h1>
    <p className='relaxed max-w-3xl text-sm opacity-70'>{t('description')}</p>
    <FirefliesSection />
    <MeetingList />
  </div>
}

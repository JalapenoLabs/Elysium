// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'

// Core
import { useTranslation } from 'react-i18next'
import { Outlet, useLocation, useNavigate } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectInboxActionItems } from '../../store/actionItemsSlice'

// User interface
import { Button, Chip, Tabs } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'

// Misc
import { useActionItemsLoader } from '../../hooks/useServerData'
import { UrlTree } from '../../urls'

const SECTIONS = [ 'next', 'inbox', 'all', 'initiatives' ] as const
type Section = typeof SECTIONS[number]

const sectionUrls = {
  next: UrlTree.actionItems,
  inbox: UrlTree.actionItemsInbox,
  all: UrlTree.actionItemsAll,
  initiatives: UrlTree.initiatives,
} as const satisfies Record<Section, string>

const sectionLabelKeys = {
  next: 'nav.next',
  inbox: 'nav.inbox',
  all: 'nav.all',
  initiatives: 'nav.initiatives',
} as const satisfies Record<Section, ParseKeys<'actionItems'>>

// The frame of the Action items area's four views: Next (the default), the inbox, every
// item, and initiatives, as tabs under one heading. Item and initiative pages, and the
// create pages, stand on their own with breadcrumbs back here.
export function ActionItemsLayout() {
  const { t } = useTranslation([ 'actionItems', 'initiatives' ])
  const navigate = useNavigate()
  const location = useLocation()
  useActionItemsLoader()
  const inboxCount = useAppSelector(selectInboxActionItems).length

  const selectedSection = SECTIONS.find((section) => sectionUrls[section] === location.pathname) ?? 'next'
  const isInitiatives = selectedSection === 'initiatives'

  return <div className='container'>
    <div className='level compact items-center'>
      <h1 className='text-3xl font-bold'>{
        t('title')
      }</h1>
      <Button
        size='sm'
        variant='outline'
        className='shrink-0'
        onPress={() => navigate(isInitiatives
          ? UrlTree.initiativesNew
          : UrlTree.actionItemsNew)}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{isInitiatives
          ? t('initiatives:newInitiative')
          : t('newItem')}</span>
      </Button>
    </div>

    <Tabs className='relaxed' variant='secondary' selectedKey={selectedSection}>
      <Tabs.ListContainer>
        <Tabs.List aria-label={t('nav.label')}>{
          SECTIONS.map((section) => <Tabs.Tab key={section} id={section} href={sectionUrls[section]}>
            <span>{t(sectionLabelKeys[section])}</span>
            {section === 'inbox' && inboxCount > 0 && <Chip size='sm' variant='soft' color='accent' className='ml-2'>{
              inboxCount
            }</Chip>}
            <Tabs.Indicator />
          </Tabs.Tab>)
        }</Tabs.List>
      </Tabs.ListContainer>
    </Tabs>

    <Outlet />
  </div>
}

// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs, Card } from '@heroui/react'

// Misc
import { UrlTree } from '../../../urls'

type Props = {
  title: string
  children: ReactNode
}

// The frame the add and edit pages share: breadcrumbs back to Storage, the title, and
// the form in a card.
export function StorageLocationEditorLayout(props: Props) {
  const { t } = useTranslation([ 'storage', 'settings' ])

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.settingsStorage}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{props.title}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      props.title
    }</h1>

    <Card className='max-w-3xl p-6'>{
      props.children
    }</Card>
  </div>
}

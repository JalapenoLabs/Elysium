// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { StorageProviderKind } from '../../../api/routes/storageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { StorageSetupChecklist } from './StorageSetupChecklist'

// Misc
import { UrlTree } from '../../../urls'

type Props = {
  title: string
  kind: StorageProviderKind
  children: ReactNode
}

// The frame the add and edit pages share: the form on the left, and on the right the
// steps for finding the provider's settings. Narrow screens stack them, form first.
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

    <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
      <section>{
        props.children
      }</section>
      <aside className='lg:sticky lg:top-4'>
        <StorageSetupChecklist kind={props.kind} />
      </aside>
    </div>
  </div>
}

// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { LlmType } from '../../../api/routes/llmRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { LlmSetupChecklist } from './LlmSetupChecklist'

// Misc
import { UrlTree } from '../../../urls'

type Props = {
  title: string
  type: LlmType
  children: ReactNode
}

// Add and edit pages share this frame: the form on the left, and on the right the
// steps for getting the token. Narrow screens stack them, form first.
export function LlmEditorLayout(props: Props) {
  const { t } = useTranslation([ 'llms', 'settings' ])

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.settingsLlms}>{t('title')}</Breadcrumbs.Item>
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
        <LlmSetupChecklist type={props.type} />
      </aside>
    </div>
  </div>
}

// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { LlmType } from '../../../api/routes/llmRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SettingsPage } from '../SettingsPage'
import { LlmSetupChecklist } from './LlmSetupChecklist'

// Misc
import { UrlTree } from '../../../urls'

type Props = {
  title: string
  type: LlmType
  // Behind the header's `?`, for a page that needs explaining.
  help?: string
  children: ReactNode
}

// Add and edit pages share this frame: the form on the left, and on the right the
// steps for getting the token. Narrow screens stack them, form first.
export function LlmEditorLayout(props: Props) {
  const { t } = useTranslation('llms')

  return <SettingsPage
    title={props.title}
    help={props.help}
    parents={[{ label: t('title'), href: UrlTree.settingsLlms }]}
  >
    <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
      <section>{
        props.children
      }</section>
      <aside className='lg:sticky lg:top-4'>
        <LlmSetupChecklist type={props.type} />
      </aside>
    </div>
  </SettingsPage>
}

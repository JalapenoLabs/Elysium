// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { SettingsPage } from '../SettingsPage'

// Misc
import { UrlTree } from '../../../urls'

type Props = {
  title: string
  children: ReactNode
}

// The frame the add and edit pages share: breadcrumbs back to environment variables and the
// title.
export function EnvironmentVariableEditorLayout(props: Props) {
  const { t } = useTranslation('environment')

  return <SettingsPage
    title={props.title}
    parents={[{ label: t('title'), href: UrlTree.settingsEnvironment }]}
  >{
      props.children
    }</SettingsPage>
}

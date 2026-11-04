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

// The frame the edit pages share: the standard settings header under Connectors, where every
// Jira site is listed. Each page lays out its own form and the setup steps beside it.
export function JiraCredentialEditorLayout(props: Props) {
  const { t } = useTranslation('connectors')

  return <SettingsPage
    title={props.title}
    parents={[{ label: t('page.title'), href: UrlTree.settingsConnectors }]}
  >{
      props.children
    }</SettingsPage>
}

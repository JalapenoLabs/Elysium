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

// The frame the edit page uses: the standard settings header under Connectors, where every
// GitHub account is listed. The form lays out itself and its kind's setup steps, since it
// knows the kind.
export function GithubCredentialEditorLayout(props: Props) {
  const { t } = useTranslation('connectors')

  return <SettingsPage
    title={props.title}
    parents={[{ label: t('page.title'), href: UrlTree.settingsConnectors }]}
  >{
      props.children
    }</SettingsPage>
}

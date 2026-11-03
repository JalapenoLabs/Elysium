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

// The frame the add and edit pages share: breadcrumbs back to Storage and the title. The
// form lays out itself and its provider's setup steps, since it knows the provider.
export function StorageLocationEditorLayout(props: Props) {
  const { t } = useTranslation('storage')

  return <SettingsPage
    title={props.title}
    parents={[{ label: t('title'), href: UrlTree.settingsStorage }]}
  >{
      props.children
    }</SettingsPage>
}

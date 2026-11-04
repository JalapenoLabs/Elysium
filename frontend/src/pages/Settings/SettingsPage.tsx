// Copyright © 2026 Jalapeno Labs

import type { ComponentProps } from 'react'
import type { PageCrumb } from '../../layout/page/PageHeader'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Page } from '../../layout/page/Page'

// Misc
import { UrlTree } from '../../urls'

type Props = Omit<ComponentProps<typeof Page>, 'escapeTo' | 'parents'> & {
  // The pages between Settings and this one, such as Manage LLMs above an LLM's edit page.
  // Esc and the back button go to the last of them, or to Settings.
  parents?: PageCrumb[]
}

// The frame of every settings subpage: the standard `Page`, under Settings in the
// breadcrumbs, where Esc goes back to the parent page. See docs/frontend.md, Settings pages.
export function SettingsPage(props: Props) {
  const { t } = useTranslation('settings')
  const parents = [{ label: t('title'), href: UrlTree.settings }, ...props.parents ?? [] ]

  return <Page
    {...props}
    parents={parents}
    escapeTo={parents.at(-1)?.href}
  />
}

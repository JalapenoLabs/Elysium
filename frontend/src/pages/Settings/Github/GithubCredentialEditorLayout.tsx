// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs } from '@heroui/react'

// Misc
import { UrlTree } from '../../../urls'

type Props = {
  title: string
  children: ReactNode
}

// The frame the add and edit pages share: breadcrumbs back to GitHub and the title. The
// form lays out itself and its kind's setup steps, since it knows the kind.
export function GithubCredentialEditorLayout(props: Props) {
  const { t } = useTranslation([ 'github', 'settings' ])

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.settings}>{t('settings:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.settingsGithub}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{props.title}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      props.title
    }</h1>
    {props.children}
  </div>
}

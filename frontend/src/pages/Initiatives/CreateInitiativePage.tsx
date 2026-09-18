// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate, useSearchParams } from 'react-router'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { InitiativeForm } from './InitiativeForm'

// Misc
import { getInitiativeViewUrl, NEW_ITEM_PROJECT_PARAM, UrlTree } from '../../urls'

// `/action-items/initiatives/new`: starting an initiative. `?project=` starts it in the
// project it was created from.
export function CreateInitiativePage() {
  const { t } = useTranslation([ 'initiatives', 'actionItems' ])
  const navigate = useNavigate()
  const [ searchParams ] = useSearchParams()
  const projectId = searchParams.get(NEW_ITEM_PROJECT_PARAM)

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.actionItems}>{t('actionItems:title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item href={UrlTree.initiatives}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('form.createTitle')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('form.createTitle')
    }</h1>
    <InitiativeForm
      initialProjectIds={projectId
        ? [ projectId ]
        : []}
      onSaved={(initiativeId) => navigate(getInitiativeViewUrl(initiativeId), { replace: true })}
      // Back to wherever the form was opened from.
      onCancel={() => navigate(-1)}
    />
  </div>
}

// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate, useSearchParams } from 'react-router'

// User interface
import { Breadcrumbs } from '@heroui/react'
import { ActionItemForm } from './ActionItemForm'

// Misc
import { getActionItemViewUrl, NEW_ITEM_INITIATIVE_PARAM, NEW_ITEM_PROJECT_PARAM, UrlTree } from '../../urls'

// `/action-items/new`: adding an item by hand. `?project=` and `?initiative=` start it in
// the project or initiative it was created from.
export function CreateActionItemPage() {
  const { t } = useTranslation('actionItems')
  const navigate = useNavigate()
  const [ searchParams ] = useSearchParams()
  const projectId = searchParams.get(NEW_ITEM_PROJECT_PARAM)
  const initiativeId = searchParams.get(NEW_ITEM_INITIATIVE_PARAM)

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.actionItems}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('form.createTitle')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='relaxed text-3xl font-bold'>{
      t('form.createTitle')
    }</h1>
    <ActionItemForm
      initialProjectIds={projectId
        ? [ projectId ]
        : []}
      initialInitiativeIds={initiativeId
        ? [ initiativeId ]
        : []}
      onSaved={(itemId) => navigate(getActionItemViewUrl(itemId), { replace: true })}
      // Back to wherever the form was opened from.
      onCancel={() => navigate(-1)}
    />
  </div>
}

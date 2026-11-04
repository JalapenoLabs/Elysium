// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate, useSearchParams } from 'react-router'

// User interface
import { Page } from '../../layout/page/Page'
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

  return <Page
    title={t('form.createTitle')}
    parents={[
      { label: t('actionItems:title'), href: UrlTree.actionItems },
      { label: t('title'), href: UrlTree.initiatives },
    ]}
  >
    <InitiativeForm
      initialProjectIds={projectId
        ? [ projectId ]
        : []}
      onSaved={(initiativeId) => navigate(getInitiativeViewUrl(initiativeId), { replace: true })}
      // Back to wherever the form was opened from.
      onCancel={() => navigate(-1)}
    />
  </Page>
}

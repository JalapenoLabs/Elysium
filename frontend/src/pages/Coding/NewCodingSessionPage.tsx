// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Page } from '../../layout/page/Page'
import { ActionItemSessionForm } from './ActionItemSessionForm'
import { CreateSessionForm } from './CreateSessionForm'
import { MissingAgentCredentialAlert } from './MissingAgentCredentialAlert'

// Misc
import { NEW_SESSION_ITEM_PARAM, UrlTree } from '../../urls'

// `/coding/new`: starting a coding session, of its own or, with `?item=`, from an action item,
// whose context its first turn carries. Creating it opens its conversation on the Coding page.
export function NewCodingSessionPage() {
  const { t } = useTranslation('coding')
  const [ searchParams ] = useSearchParams()
  const actionItemId = searchParams.get(NEW_SESSION_ITEM_PARAM)

  return <Page
    title={actionItemId
      ? t('create.fromItem.title')
      : t('create.title')}
    parents={[{ label: t('title'), href: UrlTree.coding }]}
  >
    <MissingAgentCredentialAlert />
    {actionItemId
      // Keyed by the item, so a link to another item's session starts a fresh form.
      ? <ActionItemSessionForm key={actionItemId} actionItemId={actionItemId} />
      : <CreateSessionForm actionItem={null} />}
  </Page>
}

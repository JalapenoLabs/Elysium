// Copyright © 2026 Jalapeno Labs

import type { LinkTarget } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate, useSearchParams } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'
import { actionItemLinkUpserted } from '../../store/actionItemLinksSlice'

// User interface
import { useOverlayState } from '@heroui/react'
import { LuLink } from 'react-icons/lu'
import { Page } from '../../layout/page/Page'
import { ActionItemForm } from './ActionItemForm'
import { LinkTargetModal } from './LinkTargetModal'

// Utility
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { createActionItemFromLink } from '../../api/routes/actionItemRoutes'
import { getActionItemViewUrl, NEW_ITEM_INITIATIVE_PARAM, NEW_ITEM_PROJECT_PARAM, UrlTree } from '../../urls'
import { toast } from '../../toast/toast'

// `/action-items/new`: adding an item by hand, or from a Jira issue, GitHub issue, or pull
// request it then follows. `?project=` and `?initiative=` start it in the project or
// initiative it was created from, either way.
export function CreateActionItemPage() {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const navigate = useNavigate()
  const dispatch = useAppDispatch()
  const [ searchParams ] = useSearchParams()
  const fromLinkState = useOverlayState()
  const projectId = searchParams.get(NEW_ITEM_PROJECT_PARAM)
  const initiativeId = searchParams.get(NEW_ITEM_INITIATIVE_PARAM)
  const projectIds = projectId
    ? [ projectId ]
    : []
  const initiativeIds = initiativeId
    ? [ initiativeId ]
    : []

  async function createFromLink(target: LinkTarget) {
    try {
      const response = await createActionItemFromLink({ ...target, projectIds, initiativeIds })
      dispatch(actionItemUpserted(response.item))
      dispatch(actionItemLinkUpserted(response.link))
      toast.success(t('toasts.created', { title: response.item.title }))
      navigate(getActionItemViewUrl(response.item.id), { replace: true })
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('toasts.alreadyLinked'))
        throw error
      }
      console.debug('CreateActionItemPage failed to create an item from a link', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      throw error
    }
  }

  return <Page
    title={t('form.createTitle')}
    parents={[{ label: t('title'), href: UrlTree.actionItems }]}
    actions={[{ id: 'fromLink', label: t('form.fromLink'), icon: LuLink, onAction: fromLinkState.open }]}
  >
    <ActionItemForm
      initialProjectIds={projectIds}
      initialInitiativeIds={initiativeIds}
      onSaved={(itemId) => navigate(getActionItemViewUrl(itemId), { replace: true })}
    />
    <LinkTargetModal
      state={fromLinkState}
      title={t('links.picker.createTitle')}
      description={t('links.picker.createDescription')}
      submitLabel={t('links.picker.createSubmit')}
      onSubmit={createFromLink}
    />
  </Page>
}

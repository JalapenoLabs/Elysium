// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { initiativeLinkUpserted } from '../../store/initiativeLinksSlice'

// User interface
import { Form } from '@heroui/react'
import { PageCreateFooter } from '../../layout/page/PageCreateFooter'
import { ContainerTargetPicker } from './ContainerTargetPicker'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { addInitiativeLink } from '../../api/routes/initiativeRoutes'
import { usePageSave } from '../../layout/page/pageSave'
import { getInitiativeViewUrl } from '../../urls'
import { EMPTY_CONTAINER_TARGET_DRAFT, toContainerTarget } from './containerPresentation'
import { toast } from '../../toast/toast'

type Props = {
  initiativeId: string
}

// Links an initiative to a container, whose issues then join it as items on the watcher's
// next pass, then goes back to the initiative. Following one it already follows answers that
// one unchanged, so it needs no refusal here. It follows from the page's Save, in the header
// and below the form, which waits for a container to be picked.
export function AddContainerForm(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const [ draft, setDraft ] = useState(EMPTY_CONTAINER_TARGET_DRAFT)
  const target = toContainerTarget(draft)

  async function link() {
    if (!target) {
      console.debug('AddContainerForm was saved with nothing picked')
      return false
    }
    try {
      const response = await addInitiativeLink(props.initiativeId, target)
      dispatch(initiativeLinkUpserted(response.link))
      toast.success(t('containers.linked', { name: response.link.title }))
      void navigate(getInitiativeViewUrl(props.initiativeId))
      return true
    }
    catch (error) {
      console.debug('AddContainerForm failed to link a container', { error, initiativeId: props.initiativeId })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
      return false
    }
  }

  const onSubmit = usePageSave({
    save: link,
    // Choosing where to look is work too: the search or list it opened would be lost.
    isDirty: draft !== EMPTY_CONTAINER_TARGET_DRAFT,
    blockedReason: target
      ? null
      : t('containers.pickFirst'),
    label: t('containers.addSubmit'),
  })

  return <Form onSubmit={onSubmit} className='flex max-w-xl flex-col gap-4'>
    <ContainerTargetPicker value={draft} onChange={setDraft} />
    <PageCreateFooter />
  </Form>
}

// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { initiativeLinkUpserted } from '../../store/initiativeLinksSlice'

// User interface
import { Button, Modal, toast } from '@heroui/react'
import { ContainerTargetPicker } from './ContainerTargetPicker'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { addInitiativeLink } from '../../api/routes/initiativeRoutes'
import { EMPTY_CONTAINER_TARGET_DRAFT, toContainerTarget } from './containerPresentation'

type Props = {
  initiativeId: string
  state: UseOverlayStateReturn
}

// Links an initiative to a container, whose issues then join it as items on the watcher's
// next pass. Following one it already follows answers that one unchanged, so it needs no
// refusal here.
export function AddContainerModal(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const [ draft, setDraft ] = useState(EMPTY_CONTAINER_TARGET_DRAFT)
  const [ isSaving, setIsSaving ] = useState(false)
  const target = toContainerTarget(draft)

  function close() {
    // The next opening starts over.
    setDraft(EMPTY_CONTAINER_TARGET_DRAFT)
    props.state.close()
  }

  async function link() {
    if (!target) {
      console.debug('AddContainerModal was submitted with nothing picked')
      return
    }
    setIsSaving(true)
    try {
      const response = await addInitiativeLink(props.initiativeId, target)
      dispatch(initiativeLinkUpserted(response.link))
      toast.success(t('containers.linked', { name: response.link.title }))
      close()
    }
    catch (error) {
      console.debug('AddContainerModal failed to link a container', { error, initiativeId: props.initiativeId })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsSaving(false)
    }
  }

  // Controlled overlays skip the Modal root, as in AddMailDomainModal.
  return <Modal.Backdrop
    isOpen={props.state.isOpen}
    onOpenChange={(isOpen) => {
      if (!isOpen) {
        close()
      }
    }}
  >
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('containers.addTitle')}</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{t('containers.addDescription')}</p>
        </Modal.Header>
        <Modal.Body className='mt-2'>
          <ContainerTargetPicker value={draft} onChange={setDraft} />
        </Modal.Body>
        <Modal.Footer>
          <Button slot='close' variant='tertiary'>
            <span>{t('common:actions.cancel')}</span>
          </Button>
          <Button isDisabled={!target} isPending={isSaving} onPress={() => void link()}>
            <span>{t('containers.addSubmit')}</span>
          </Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}

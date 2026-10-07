// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Modal } from '@heroui/react'
import { CreateStudioItemForm } from './CreateStudioItemForm'

type Props = {
  state: UseOverlayStateReturn
}

// The New item dialog.
export function CreateStudioItemModal(props: Props) {
  const { t } = useTranslation('studio')

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-2xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('create.title')}</Modal.Heading>
        </Modal.Header>
        <CreateStudioItemForm onCreated={props.state.close} />
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}

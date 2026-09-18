// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Modal } from '@heroui/react'
import { ActionItemSessionForm } from './ActionItemSessionForm'
import { CreateSessionForm } from './CreateSessionForm'

type Props = {
  state: UseOverlayStateReturn
  // The action item the session is started from, or null for a session of its own.
  actionItemId: string | null
}

// The New session dialog, for a session of its own or one started from an action item.
export function CreateSessionModal(props: Props) {
  const { t } = useTranslation('coding')

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-2xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            props.actionItemId
              ? t('create.fromItem.title')
              : t('create.title')
          }</Modal.Heading>
        </Modal.Header>
        {props.actionItemId
          ? <ActionItemSessionForm
            actionItemId={props.actionItemId}
            onCreated={props.state.close}
          />
          : <CreateSessionForm
            actionItem={null}
            onCreated={props.state.close}
          />}
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}

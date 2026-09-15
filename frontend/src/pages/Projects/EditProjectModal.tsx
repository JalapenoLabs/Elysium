// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Modal } from '@heroui/react'
import { ProjectForm } from './ProjectForm'

type Props = {
  state: UseOverlayStateReturn
  // Null only while no project has been chosen; the dialog is closed then.
  project: Project | null
}

export function EditProjectModal(props: Props) {
  const { t } = useTranslation('projects')

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('form.editTitle', { name: props.project?.name ?? '' })
          }</Modal.Heading>
        </Modal.Header>
        <Modal.Body className='mt-2'>
          {props.project && <ProjectForm
            project={props.project}
            layout='stacked'
            onSaved={props.state.close}
            onCancel={props.state.close}
          />}
        </Modal.Body>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}

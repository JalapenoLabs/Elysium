// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectDeleted } from '../../store/projectsSlice'

// User interface
import { AlertDialog, Button, toast } from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import { deleteProject } from '../../api/routes/projectRoutes'

type Props = {
  state: UseOverlayStateReturn
  project: Project | null
  // Sessions this browser knows belong to the project. The API has the final say.
  sessionCount: number
}

export function DeleteProjectDialog(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const [ isDeleting, setIsDeleting ] = useState(false)
  const name = props.project?.name ?? ''

  async function onConfirm() {
    if (!props.project) {
      console.debug('DeleteProjectDialog confirmed with no project selected')
      return
    }

    setIsDeleting(true)
    try {
      await deleteProject(props.project.id)
      dispatch(projectDeleted(props.project.id))
      toast.success(t('toasts.deleted', { name: props.project.name }))
      props.state.close()
    }
    catch (error) {
      // A session created elsewhere since this list loaded.
      if (error instanceof HTTPError && error.response.status === 409) {
        toast.danger(t('toasts.stillHasSessions', { name: props.project.name }))
        return
      }

      console.debug('DeleteProjectDialog failed to delete the project', { error })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsDeleting(false)
    }
  }

  const hasSessions = props.sessionCount > 0

  // Controlled overlays skip the AlertDialog root: it is a trigger wrapper, and without
  // a pressable child React Aria warns. The backdrop takes the open state directly.
  return <AlertDialog.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <AlertDialog.Container>
      <AlertDialog.Dialog className='sm:max-w-md'>
        <AlertDialog.Header>
          <AlertDialog.Icon status='danger' />
          <AlertDialog.Heading>{
            t('delete.title', { name })
          }</AlertDialog.Heading>
        </AlertDialog.Header>
        <AlertDialog.Body>
          <p>{
            hasSessions
              ? t('delete.hasSessions', { name, count: props.sessionCount })
              : t('delete.body')
          }</p>
        </AlertDialog.Body>
        <AlertDialog.Footer>
          <Button slot='close' variant='tertiary'>
            <span>{t('common:actions.cancel')}</span>
          </Button>
          {/* Nothing to confirm while sessions remain: the API would refuse. */}
          {!hasSessions && <Button
            variant='danger'
            isPending={isDeleting}
            onPress={onConfirm}
          >
            <span>{t('common:actions.delete')}</span>
          </Button>}
        </AlertDialog.Footer>
      </AlertDialog.Dialog>
    </AlertDialog.Container>
  </AlertDialog.Backdrop>
}

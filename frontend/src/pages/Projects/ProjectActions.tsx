// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectDeleted, projectUpserted } from '../../store/projectsSlice'

// User interface
import { Button, Dropdown, Label, toast } from '@heroui/react'
import { LuChevronDown, LuImageOff, LuTrash2 } from 'react-icons/lu'

// Utility
import { HTTPError } from 'ky'

// Misc
import { deleteProject, deleteProjectCover } from '../../api/routes/projectRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { UrlTree } from '../../urls'

type Props = {
  project: Project
  // Sessions this browser knows belong to the project. The API has the final say.
  sessionCount: number
}

// The project page's "Actions" menu: what the page cannot do in place. The name,
// description, and a new cover are edited where they are shown.
export function ProjectActions(props: Props) {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()

  function confirmDelete() {
    const name = props.project.name
    const hasSessions = props.sessionCount > 0

    confirm({
      title: t('delete.title', { name }),
      message: hasSessions
        ? t('delete.hasSessions', { name, count: props.sessionCount })
        : t('delete.body'),
      tone: 'danger',
      confirmText: t('delete.confirm'),
      // Nothing to confirm while sessions remain: the API would refuse.
      canConfirm: !hasSessions,
      onConfirm: async () => {
        try {
          await deleteProject(props.project.id)
        }
        catch (error) {
          // A session created elsewhere since this page loaded.
          if (error instanceof HTTPError && error.response.status === 409) {
            toast.danger(t('toasts.stillHasSessions', { name }))
            return
          }
          console.debug('ProjectActions failed to delete the project', { error, projectId: props.project.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }

        // Leave first, so this page never renders the project as missing.
        navigate(UrlTree.projects)
        dispatch(projectDeleted(props.project.id))
        toast.success(t('toasts.deleted', { name }))
      },
    })
  }

  async function removeCover() {
    try {
      const response = await deleteProjectCover(props.project.id)
      dispatch(projectUpserted(response.project))
      toast.success(t('toasts.coverRemoved'))
    }
    catch (error) {
      console.debug('ProjectActions failed to remove the cover', { error, projectId: props.project.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  const actions: Record<string, () => void> = {
    removeCover: () => void removeCover(),
    delete: confirmDelete,
  }

  return <Dropdown>
    <Button variant='outline'>
      <span>{t('page.actions')}</span>
      <LuChevronDown className='size-4' aria-hidden />
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu
        aria-label={t('page.actions')}
        disabledKeys={props.project.coverUpdatedAt
          ? []
          : [ 'removeCover' ]}
        onAction={(key: Key) => actions[String(key)]?.()}
      >
        <Dropdown.Item id='removeCover' textValue={t('page.removeCover')}>
          <LuImageOff className='size-4' aria-hidden />
          <Label>{t('page.removeCover')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
          <LuTrash2 className='size-4' aria-hidden />
          <Label>{t('common:actions.delete')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}

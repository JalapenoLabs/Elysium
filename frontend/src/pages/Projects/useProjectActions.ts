// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'
import type { PageAction } from '../../layout/page/pageActions'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectDeleted, projectUpserted } from '../../store/projectsSlice'

// User interface
import { LuBrainCircuit, LuContainer, LuImageOff, LuTrash2 } from 'react-icons/lu'

// Utility
import { HTTPError } from 'ky'

// Misc
import { deleteProject, deleteProjectCover, restoreProjectCover } from '../../api/routes/projectRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { LEAVE_WITHOUT_ASKING } from '../../layout/page/pageSave'
import { getProjectModelsUrl, getProjectWorkersUrl, UrlTree } from '../../urls'
import { toast } from '../../toast/toast'

type Options = {
  project: Project
  // Sessions this browser knows belong to the project. The API has the final say.
  sessionCount: number
}

// The project page's actions, its Actions menu: what the page cannot do in place. The name,
// description, and a new cover are edited where they are shown; the models its agents run on
// and how its workers' containers are readied are pages of their own.
export function useProjectActions(options: Options): PageAction[] {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()
  const [ isRemovingCover, setIsRemovingCover ] = useState(false)

  function confirmDelete() {
    const name = options.project.name
    const hasSessions = options.sessionCount > 0

    confirm({
      title: t('delete.title', { name }),
      message: hasSessions
        ? t('delete.hasSessions', { name, count: options.sessionCount })
        : t('delete.body', { name }),
      isIrreversible: true,
      confirmText: t('delete.confirm'),
      // Nothing to confirm while sessions remain: the API would refuse.
      canConfirm: !hasSessions,
      onConfirm: async () => {
        try {
          await deleteProject(options.project.id)
        }
        catch (error) {
          // A session created elsewhere since this page loaded.
          if (error instanceof HTTPError && error.response.status === 409) {
            toast.danger(t('toasts.stillHasSessions', { name }))
            return
          }
          console.debug('useProjectActions failed to delete the project', { error, projectId: options.project.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }

        // Leave first, so this page never renders the project as missing. Its unsaved edits
        // went with it, so leaving asks nothing.
        void navigate(UrlTree.projects, { state: LEAVE_WITHOUT_ASKING })
        dispatch(projectDeleted(options.project.id))
        toast.success(t('toasts.deleted', { name }))
      },
    })
  }

  // The API keeps a removed cover until a new one replaces it, so removing happens at once,
  // with Undo in its toast.
  async function removeCover() {
    const projectId = options.project.id
    setIsRemovingCover(true)
    try {
      const response = await deleteProjectCover(projectId)
      dispatch(projectUpserted(response.project))
      toast.success(t('toasts.coverRemoved'), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void restoreCover(projectId),
        },
      })
    }
    catch (error) {
      console.debug('useProjectActions failed to remove the cover', { error, projectId })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsRemovingCover(false)
    }
  }

  async function restoreCover(projectId: string) {
    try {
      const response = await restoreProjectCover(projectId)
      dispatch(projectUpserted(response.project))
      toast.success(t('toasts.coverRestored'))
    }
    catch (error) {
      console.debug('useProjectActions failed to restore the cover', { error, projectId })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  return [
    {
      id: 'models',
      label: t('page.models'),
      icon: LuBrainCircuit,
      href: getProjectModelsUrl(options.project.id),
    },
    {
      id: 'workers',
      label: t('page.workers'),
      icon: LuContainer,
      href: getProjectWorkersUrl(options.project.id),
    },
    {
      id: 'removeCover',
      label: t('page.removeCover'),
      icon: LuImageOff,
      isDisabled: !options.project.coverUpdatedAt,
      isPending: isRemovingCover,
      onAction: () => void removeCover(),
    },
    {
      id: 'delete',
      label: t('common:actions.delete'),
      icon: LuTrash2,
      isDanger: true,
      onAction: confirmDelete,
    },
  ]
}

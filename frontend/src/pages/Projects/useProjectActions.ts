// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'
import type { PageAction } from '../../layout/page/pageActions'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { projectDeleted, projectUpserted } from '../../store/projectsSlice'

// User interface
import { LuImageOff, LuTrash2 } from 'react-icons/lu'

// Utility
import { HTTPError } from 'ky'

// Misc
import { deleteProject, deleteProjectCover } from '../../api/routes/projectRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { LEAVE_WITHOUT_ASKING } from '../../layout/page/pageSave'
import { UrlTree } from '../../urls'
import { toast } from '../../toast/toast'

type Options = {
  project: Project
  // Sessions this browser knows belong to the project. The API has the final say.
  sessionCount: number
}

// The project page's actions, its Actions menu: what the page cannot do in place. The name,
// description, and a new cover are edited where they are shown.
export function useProjectActions(options: Options): PageAction[] {
  const { t } = useTranslation([ 'projects', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()

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

  async function removeCover() {
    try {
      const response = await deleteProjectCover(options.project.id)
      dispatch(projectUpserted(response.project))
      toast.success(t('toasts.coverRemoved'))
    }
    catch (error) {
      console.debug('useProjectActions failed to remove the cover', { error, projectId: options.project.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  return [
    {
      id: 'removeCover',
      label: t('page.removeCover'),
      icon: LuImageOff,
      isDisabled: !options.project.coverUpdatedAt,
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

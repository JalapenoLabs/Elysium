// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { pageLoaded } from '../../../store/pagesSlice'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { linkPageToProject, projectPagesKey, unlinkPageFromProject } from '../../../api/routes/pageRoutes'
import { toast } from '../../../toast/toast'

// One link, named as the person reads it, since either end may be off the screen they are on.
export type PageProjectLink = {
  pageId: string
  pageTitle: string
  projectId: string
  projectName: string
}

// Links a page to a project and takes it off, from either end. A link is an action, not a field of
// the page or the project, so it applies at once, away from their Saves, and its toast offers Undo.
// What it answers feeds the page, and the project's list loads again, since a list of a project's
// pages holds more than a page says. Each resolves once the toast is up, and never rejects.
export function usePageProjectLinks() {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const dispatch = useAppDispatch()

  function settle(page: KnowledgePage, projectId: string) {
    dispatch(pageLoaded(page))
    void mutate(projectPagesKey(projectId))
  }

  async function link(target: PageProjectLink) {
    const { pageId, pageTitle, projectId, projectName } = target
    try {
      const response = await linkPageToProject(pageId, projectId)
      settle(response.page, projectId)
      toast.success(t('projectLinks.linked', { page: pageTitle, project: projectName }), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void unlink(target),
        },
      })
    }
    catch (error) {
      console.debug('usePageProjectLinks failed to link a page', { error, pageId, projectId })
      toast.danger(t('projectLinks.linkFailed'), { description: getApiErrorMessage(error) })
    }
  }

  async function unlink(target: PageProjectLink) {
    const { pageId, pageTitle, projectId, projectName } = target
    try {
      const response = await unlinkPageFromProject(pageId, projectId)
      settle(response.page, projectId)
      toast.success(t('projectLinks.unlinked', { page: pageTitle, project: projectName }), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void link(target),
        },
      })
    }
    catch (error) {
      console.debug('usePageProjectLinks failed to take a page off', { error, pageId, projectId })
      toast.danger(t('projectLinks.unlinkFailed'), { description: getApiErrorMessage(error) })
    }
  }

  return { link, unlink } as const
}

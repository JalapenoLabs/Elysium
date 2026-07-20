// Copyright © 2026 Jalapeno Labs

import type { PaletteEntry } from './registry'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { dispatch } from '../../store'
import { useAppSelector } from '../../store/hooks'
import { activeProjectChanged, activeProjectStorageKey } from '../../store/projectsSlice'

// Misc
import { usePaletteEntries } from './registry'

// Every project becomes a "Switch to <project>" palette entry, so changing the
// working project never needs the mouse (docs/ux.md, keyboard-first).
export function useProjectPaletteEntries() {
  const { t } = useTranslation()

  const projects = useAppSelector((state) => state.projects.projects)
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)

  const entries = useMemo<PaletteEntry[]>(() => {
    if (!activeOrgId) {
      return []
    }
    return projects.map((project) => ({
      id: `project.switch.${project.id}`,
      label: t('palette.switchProject', { name: project.name }),
      keywords: project.slug,
      section: t('palette.sections.project'),
      run: () => {
        dispatch(activeProjectChanged(project.id))
        localStorage.setItem(activeProjectStorageKey(activeOrgId), project.id)
      },
    }))
  }, [ projects, activeOrgId, t ])

  usePaletteEntries('app.projects', entries)
}

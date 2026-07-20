// Copyright © 2026 Jalapeno Labs

import type { WorkspacePanelDefinition } from './registry'

// Core
import { createElement, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { LuFolderOpen } from 'react-icons/lu'
import { About } from '../pages/About/About'
import { FilesPanel } from '../drive/FilesPanel'

// Misc
import { useWorkspacePanels } from './registry'

// The panels every deployment has, registered from the app shell. Pillars
// (drive, git, mail, the Claude session) register theirs the same way as they
// land; the shell and this list stay unaware of each other's internals.
export function useCoreWorkspacePanels() {
  const { t } = useTranslation()

  const definitions = useMemo<WorkspacePanelDefinition[]>(() => [
    {
      id: 'files',
      kind: 'component',
      label: t('drive.files.panelTitle'),
      icon: createElement(LuFolderOpen, { className: 'h-5 w-5' }),
      render: () => createElement(FilesPanel),
    },
    {
      id: 'about',
      kind: 'component',
      label: t('nav.about'),
      render: () => createElement(About),
    },
  ], [ t ])

  useWorkspacePanels('app.core', definitions)
}

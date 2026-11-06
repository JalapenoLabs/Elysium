// Copyright © 2026 Jalapeno Labs

import type { ProjectPage } from '../../api/routes/pageRoutes'
import type { AddPickerOption } from '../../components/AddPicker'
import type { LoadStatus } from '../../hooks/useServerData'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectPageTree } from '../../store/pagesSlice'
import { selectProjectById } from '../../store/projectsSlice'

// User interface
import { Spinner } from '@heroui/react'
import { AddPicker } from '../../components/AddPicker'
import { EmptyNotice } from '../../components/EmptyNotice'
import { ProjectPagesTable } from './ProjectPagesTable'

// Misc
import { usePendingAction } from '../../hooks/usePendingAction'
import { usePagesLoader } from '../../hooks/useServerData'
import { listPageDestinations } from '../Knowledge/Pages/pageTree'
import { usePageProjectLinks } from '../Knowledge/Pages/usePageProjectLinks'

type Props = {
  projectId: string
  status: LoadStatus
  pages: ProjectPage[]
}

// A project's Docs tab: the knowledge base pages linked to it, and the field that links another,
// picked from the tree by title or by where it sits. The panel loads the tree itself, so a project
// page opened on another tab never pays for it. The table's search and export share the field's row.
export function ProjectPages(props: Props) {
  const { projectId, pages } = props
  const { t } = useTranslation('projects')
  usePagesLoader()
  const tree = useAppSelector(selectPageTree)
  const project = useAppSelector((state) => selectProjectById(state, projectId))
  const links = usePageProjectLinks()
  const pending = usePendingAction<string>()
  const [ controlsContainer, setControlsContainer ] = useState<HTMLDivElement | null>(null)

  const projectName = project?.name ?? ''
  const linkedIds = new Set(pages.map((page) => page.id))
  const addable: AddPickerOption[] = []
  for (const page of listPageDestinations(tree, null)) {
    if (!linkedIds.has(page.id)) {
      addable.push({ id: page.id, label: page.title, detail: page.ancestorTitles.join(' / ') })
    }
  }

  function add(pageId: string) {
    const pageTitle = tree.byId.get(pageId)?.title
    if (!pageTitle) {
      console.debug('ProjectPages was asked to link a page the tree no longer holds', { pageId })
      return
    }
    void pending.run(pageId, () => links.link({ pageId, pageTitle, projectId, projectName }))
  }

  return <>
    <div className='compact flex flex-wrap items-center justify-end gap-2'>
      <div ref={setControlsContainer} className='min-w-0 flex-1' />
      <AddPicker
        id='project-page-add'
        className='w-full sm:w-72'
        label={t('docs.add')}
        emptyLabel={t('docs.noneToAdd')}
        options={addable}
        isDisabled={pending.isAnyPending}
        onPick={add}
      />
    </div>
    {props.status === 'loading' && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}
    {props.status === 'failed' && <EmptyNotice>{t('docs.loadError')}</EmptyNotice>}
    {props.status === 'loaded' && !pages.length && <EmptyNotice>{t('docs.empty')}</EmptyNotice>}
    {pages.length > 0 && <ProjectPagesTable
      projectId={projectId}
      projectName={projectName}
      pages={pages}
      tree={tree}
      controlsContainer={controlsContainer}
    />}
  </>
}

// Copyright © 2026 Jalapeno Labs

import type { ProjectSort, ProjectView } from './projectListing'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { shallowEqual } from 'react-redux'
import { selectSessionCountsByProjectId } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectAllProjects } from '../../store/projectsSlice'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { ListSearchField } from '../../components/ListSearchField'
import { Page } from '../../layout/page/Page'
import { ProjectsToolbar } from './ProjectsToolbar'
import { ProjectTable } from './ProjectTable'
import { ProjectTiles } from './ProjectTiles'

// Misc
import { PROJECTS_VIEW_STORAGE_KEY } from '../../constants'
import { UrlTree } from '../../urls'
import { useCodingSessionsLoader, useProjectsLoader } from '../../hooks/useServerData'
import { DEFAULT_PROJECT_SORT, PROJECT_VIEWS, searchAndSortProjects } from './projectListing'

// The view is a per-browser convenience: storage may be unavailable, and anything
// unrecognized falls back to the table.
function readStoredView(): ProjectView {
  try {
    const stored = window.localStorage.getItem(PROJECTS_VIEW_STORAGE_KEY)
    return PROJECT_VIEWS.find((view) => view === stored) ?? 'table'
  }
  catch (error) {
    console.debug('The projects view could not be read from localStorage', { error })
    return 'table'
  }
}

// `/projects`: every project, as a table or tiles. The titlebar's search and the toolbar's sort
// drive both views; the table puts its Columns and export in the titlebar beside the search.
export function ProjectsPage() {
  const { t } = useTranslation('projects')
  const projects = useAppSelector(selectAllProjects)
  const status = useProjectsLoader()
  useCodingSessionsLoader()
  const sessionCounts = useAppSelector(selectSessionCountsByProjectId, shallowEqual)

  const [ search, setSearch ] = useState('')
  const [ sort, setSort ] = useState<ProjectSort>(DEFAULT_PROJECT_SORT)
  const [ view, setView ] = useState<ProjectView>(readStoredView)

  const listedProjects = useMemo(
    () => searchAndSortProjects(projects, sessionCounts, search, sort),
    [ projects, sessionCounts, search, sort ],
  )

  function changeView(nextView: ProjectView) {
    setView(nextView)
    try {
      window.localStorage.setItem(PROJECTS_VIEW_STORAGE_KEY, nextView)
    }
    catch (error) {
      console.debug('The projects view could not be saved to localStorage', { error })
    }
  }

  const hasProjects = status === 'loaded' && projects.length > 0

  return <Page
    title={t('title')}
    tools={hasProjects && <ListSearchField
      label={t('toolbar.search')}
      value={search}
      onChange={setSearch}
    />}
    primaryAction={<Link href={UrlTree.projectsNew} className={buttonVariants({ variant: 'primary' })}>
      <LuPlus className='size-4' aria-hidden />
      <span>{t('add')}</span>
    </Link>}
  >

    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}

    {status === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      t('table.loadError')
    }</p>}

    {status === 'loaded' && !projects.length && <p
      className='rounded-xl border border-separator py-10 text-center text-sm opacity-70'
    >{
      t('table.empty')
    }</p>}

    {hasProjects && <>
      <ProjectsToolbar
        sort={sort}
        onSortChange={setSort}
        view={view}
        onViewChange={changeView}
        resultsCount={listedProjects.length}
      />

      {!listedProjects.length && <p className='py-10 text-center text-sm opacity-70'>{
        t('table.noMatches')
      }</p>}

      {listedProjects.length > 0 && view === 'table' && <ProjectTable
        projects={listedProjects}
        sort={sort}
        onSortChange={setSort}
        sessionCounts={sessionCounts}
      />}

      {listedProjects.length > 0 && view === 'tiles' && <ProjectTiles
        projects={listedProjects}
        sessionCounts={sessionCounts}
      />}
    </>}
  </Page>
}

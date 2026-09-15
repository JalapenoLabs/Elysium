// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

// User interface
import { Card } from '@heroui/react'
import { ProjectCover } from './ProjectCover'

// Misc
import { getProjectCoverUrl } from '../../api/routes/projectRoutes'
import { getProjectViewUrl } from '../../urls'

// The whole card is the link. Its corners follow the card's, so the focus ring does too.
const TILE_LINK_CLASS_NAME = [
  'block h-full rounded-[min(32px,var(--radius-3xl))] transition-transform hover:-translate-y-0.5',
  'focus-visible:ring-2 focus-visible:ring-focus focus-visible:outline-none',
].join(' ')

type Props = {
  projects: Project[]
  sessionCounts: Record<string, number>
}

// The tiles view: one card per project, in the order the toolbar chose. Each card is a
// link to its project.
export function ProjectTiles(props: Props) {
  const { t, i18n } = useTranslation('projects')

  // Timestamps arrive as UTC; this is where they become the viewer's local time.
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })

  return <ul className='grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3'>{
    props.projects.map((project) => <li key={project.id}>
      <Link to={getProjectViewUrl(project.id)} className={TILE_LINK_CLASS_NAME}>
        <Card className='h-full'>
          <ProjectCover
            src={getProjectCoverUrl(project)}
            name={project.name}
            className='aspect-video w-full rounded-2xl'
          />
          <Card.Header>
            <Card.Title className='min-w-0 truncate'>{project.name}</Card.Title>
          </Card.Header>
          <Card.Content>
            <p className='line-clamp-3 text-sm opacity-70'>{
              project.description || t('tile.noDescription')
            }</p>
          </Card.Content>
          <Card.Footer className='flex flex-wrap justify-between gap-2 text-xs opacity-70'>
            <span>{t('tile.sessions', { count: props.sessionCounts[project.id] ?? 0 })}</span>
            <span>{t('tile.updated', { date: dateFormatter.format(new Date(project.updatedAt)) })}</span>
          </Card.Footer>
        </Card>
      </Link>
    </li>)
  }</ul>
}

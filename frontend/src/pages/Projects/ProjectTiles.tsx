// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Card } from '@heroui/react'
import { ProjectRowActions } from './ProjectRowActions'

type Props = {
  projects: Project[]
  sessionCounts: Record<string, number>
  onEdit: (project: Project) => void
  onDelete: (project: Project) => void
}

// The tiles view: one card per project, in the order the toolbar chose.
export function ProjectTiles(props: Props) {
  const { t, i18n } = useTranslation('projects')

  // Timestamps arrive as UTC; this is where they become the viewer's local time.
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })

  return <ul className='grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3'>{
    props.projects.map((project) => <li key={project.id}>
      <Card className='h-full'>
        <Card.Header className='flex flex-row items-start justify-between gap-2'>
          <Card.Title className='min-w-0 truncate'>{project.name}</Card.Title>
          <ProjectRowActions
            project={project}
            onEdit={props.onEdit}
            onDelete={props.onDelete}
          />
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
    </li>)
  }</ul>
}

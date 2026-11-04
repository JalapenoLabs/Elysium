// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { Card } from '@heroui/react'
import { Page } from '../../layout/page/Page'
import { ProjectForm } from './ProjectForm'

// Misc
import { UrlTree } from '../../urls'

// `/projects/new`: creating a project, with room to preview its cover at tile size.
export function CreateProjectPage() {
  const { t } = useTranslation('projects')
  const navigate = useNavigate()

  return <Page
    title={t('form.createTitle')}
    parents={[{ label: t('title'), href: UrlTree.projects }]}
  >
    <Card className='max-w-4xl p-6'>
      <ProjectForm
        onSaved={() => navigate(UrlTree.projects)}
        onCancel={() => navigate(UrlTree.projects)}
      />
    </Card>
  </Page>
}

// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// User interface
import { Breadcrumbs, Card } from '@heroui/react'
import { ProjectForm } from './ProjectForm'

// Misc
import { UrlTree } from '../../urls'

// `/projects/new`: creating a project, with room to preview its cover at tile size.
export function CreateProjectPage() {
  const { t } = useTranslation('projects')
  const navigate = useNavigate()

  return <div className='container'>
    <Breadcrumbs className='compact'>
      <Breadcrumbs.Item href={UrlTree.projects}>{t('title')}</Breadcrumbs.Item>
      <Breadcrumbs.Item>{t('form.createTitle')}</Breadcrumbs.Item>
    </Breadcrumbs>
    <h1 className='text-3xl font-bold'>{
      t('form.createTitle')
    }</h1>
    <p className='relaxed mt-1 max-w-2xl text-sm opacity-70'>{
      t('form.createDescription')
    }</p>

    <Card className='max-w-4xl p-6'>
      <ProjectForm
        onSaved={() => navigate(UrlTree.projects)}
        onCancel={() => navigate(UrlTree.projects)}
      />
    </Card>
  </div>
}

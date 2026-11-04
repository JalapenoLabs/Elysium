// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'
import type { LoadStatus } from '../../hooks/useServerData'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuPlus } from 'react-icons/lu'
import { EmptyNotice } from '../../components/EmptyNotice'
import { InitiativeTable } from '../Initiatives/InitiativeTable'

// Misc
import { getNewInitiativeUrl } from '../../urls'

type Props = {
  projectId: string
  status: LoadStatus
  initiatives: Initiative[]
}

// A project's Initiatives tab: the initiatives it holds, and a way to start one here.
export function ProjectInitiatives(props: Props) {
  const { t } = useTranslation('projects')

  return <>
    <div className='level-right compact justify-end'>
      <Link
        href={getNewInitiativeUrl(props.projectId)}
        className={buttonVariants({ size: 'sm', variant: 'outline', className: 'gap-2' })}
      >
        <LuPlus className='size-4' aria-hidden />
        <span>{t('work.newInitiative')}</span>
      </Link>
    </div>
    {props.status === 'loading' && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}
    {props.status !== 'loading' && !props.initiatives.length && <EmptyNotice>{
      t('work.initiativesEmpty')
    }</EmptyNotice>}
    {props.initiatives.length > 0 && <InitiativeTable
      initiatives={props.initiatives}
      label={t('work.initiativesHeading')}
      storageId='elysium.projects.initiatives.table.v1'
    />}
  </>
}

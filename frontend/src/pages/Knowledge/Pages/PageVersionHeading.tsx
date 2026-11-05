// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { KnowledgePage, PageVersion } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { RestoreVersionButton } from './RestoreVersionButton'

type Props = {
  page: KnowledgePage
  version: PageVersion
  // Beside its name, such as a link to its other view.
  action?: ReactNode
  // Below it, whatever the view says about the version.
  children?: ReactNode
}

// Which version this is, what it was called if the page has been renamed since, and Restore unless
// the page already reads as it.
export function PageVersionHeading(props: Props) {
  const { t } = useTranslation('knowledge')
  const { page, version } = props
  const isCurrent = version.version === page.currentVersion

  return <div className='min-w-0'>
    <div className='flex min-h-8 flex-wrap items-center gap-x-2 gap-y-1'>
      <h2 className='text-sm font-semibold'>{t('pages.history.version', { version: version.version })}</h2>
      {isCurrent && <Chip size='sm' variant='soft' color='accent'>{t('pages.history.current')}</Chip>}
      {props.action}
      {!isCurrent && !page.deletedAt && <RestoreVersionButton pageId={page.id} version={version.version} />}
    </div>
    {version.title !== page.title && <p className='text-sm'>{version.title}</p>}
    {props.children}
  </div>
}

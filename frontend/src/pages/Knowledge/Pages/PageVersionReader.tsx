// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWRImmutable from 'swr/immutable'

// User interface
import { buttonVariants, Link, Spinner } from '@heroui/react'
import { LuGitCompareArrows } from 'react-icons/lu'
import { LearningFields } from './LearningFields'
import { PageArticle } from './PageArticle'
import { PageByline } from './PageByline'
import { PageVersionHeading } from './PageVersionHeading'

// Misc
import { getPageVersion, pageVersionKey } from '../../../api/routes/pageRoutes'
import { getPageCompareUrl } from '../../../urls'

type Props = {
  page: KnowledgePage
  version: number
}

// One version as it read, in the page's own layout, with who published it. A page's children are no
// part of a version, so they stay on the Page tab.
export function PageVersionReader(props: Props) {
  const { t } = useTranslation('knowledge')
  const { page, version } = props
  const loaded = useSWRImmutable(
    pageVersionKey(page.id, version),
    () => getPageVersion(page.id, version),
    { shouldRetryOnError: false },
  )
  const read = loaded.data?.version

  if (loaded.error) {
    return <p className='text-sm opacity-70'>{t('pages.history.versionUnavailable')}</p>
  }
  if (!read) {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }

  return <div>
    <div className='compact'>
      <PageVersionHeading
        page={page}
        version={read}
        action={version > 1 && <Link
          href={getPageCompareUrl(page.id, version - 1, version)}
          className={buttonVariants({ size: 'sm', variant: 'ghost' })}
        >
          <LuGitCompareArrows className='size-4' aria-hidden />
          <span>{t('pages.history.changes')}</span>
        </Link>}
      />
    </div>
    <PageArticle
      markdown={read.body}
      byline={<PageByline page={{ publishedBy: read.createdBy, publishedAt: read.createdAt, agent: read.agent }} />}
      learning={read.category && read.evidence
        ? <LearningFields category={read.category} evidence={read.evidence} />
        : undefined}
    />
  </div>
}

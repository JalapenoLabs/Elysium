// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { KnowledgePage, PageVersion } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'
import useSWRImmutable from 'swr/immutable'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { TextDiff } from '../../../components/TextDiff'
import { PageByline } from './PageByline'
import { PageVersionHeading } from './PageVersionHeading'

// Misc
import { getPageVersion, pageVersionKey } from '../../../api/routes/pageRoutes'
import { useLearningCategories } from '../../../hooks/useLearningCategories'

type Props = {
  page: KnowledgePage
  // The older of the two.
  from: number
  to: number
}

const FIELDS = [ 'title', 'category', 'body', 'evidence' ] as const
type Field = typeof FIELDS[number]

const fieldLabelKeys = {
  title: 'pages.history.fields.title',
  category: 'pages.history.fields.category',
  body: 'pages.history.fields.body',
  evidence: 'pages.history.fields.evidence',
} as const satisfies Record<Field, ParseKeys<'knowledge'>>

// Two versions side by side, then what differs between them, field by field: a learning's revision
// may touch only its evidence, and a page's only its title. Only these two bodies load, and a
// version never changes, so comparing it again asks for nothing.
export function PageVersionCompare(props: Props) {
  const { t } = useTranslation('knowledge')
  const { page, from, to } = props
  const { labels } = useLearningCategories()
  const older = useSWRImmutable(
    pageVersionKey(page.id, from),
    () => getPageVersion(page.id, from),
    { shouldRetryOnError: false },
  )
  const newer = useSWRImmutable(
    pageVersionKey(page.id, to),
    () => getPageVersion(page.id, to),
    { shouldRetryOnError: false },
  )
  const before = older.data?.version
  const after = newer.data?.version

  if (older.error || newer.error) {
    return <p className='text-sm opacity-70'>{t('pages.history.versionUnavailable')}</p>
  }
  if (!before || !after) {
    return <div className='grid place-items-center py-6'>
      <Spinner size='sm' />
    </div>
  }

  // A category reads as people know it, and one removed since as its key, as everywhere else.
  function fieldText(version: PageVersion, field: Field) {
    const text = version[field] ?? ''
    if (field === 'category') {
      return labels[text] ?? text
    }
    return text
  }

  const changedFields = FIELDS.filter((field) => before[field] !== after[field])

  return <div>
    <div className='relaxed grid gap-4 sm:grid-cols-2'>{
      [ before, after ].map((version) => <PageVersionHeading key={version.version} page={page} version={version}>
        <PageByline
          page={{ publishedBy: version.createdBy, publishedAt: version.createdAt, agent: version.agent }}
        />
      </PageVersionHeading>)
    }</div>
    {!changedFields.length && <EmptyNotice>{t('pages.history.same')}</EmptyNotice>}
    {changedFields.map((field) => <section key={field} className='relaxed'>
      <h3 className='compact text-sm font-semibold'>{t(fieldLabelKeys[field])}</h3>
      <TextDiff
        before={fieldText(before, field)}
        after={fieldText(after, field)}
        label={t(fieldLabelKeys[field])}
        foldedLabel={(count) => t('pages.history.foldedLines', { count })}
      />
    </section>)}
  </div>
}

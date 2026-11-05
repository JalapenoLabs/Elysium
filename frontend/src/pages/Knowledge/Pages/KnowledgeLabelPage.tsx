// Copyright © 2026 Jalapeno Labs

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'

// Redux
import { useAppSelector } from '../../../store/hooks'
import { selectPageTree } from '../../../store/pagesSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../../components/EmptyNotice'
import { Page } from '../../../layout/page/Page'
import { LabelPagesTable } from './LabelPagesTable'

// Misc
import { usePagesLoader } from '../../../hooks/useServerData'
import { UrlTree } from '../../../urls'

// `/knowledge/labels/:label`: every live page carrying a label, from the tree the frame already
// loaded. A label nothing carries any more reads as an empty list, since a link to it may outlive it.
export function KnowledgeLabelPage() {
  const { t } = useTranslation('knowledge')
  const { label = '' } = useParams()
  const status = usePagesLoader()
  const tree = useAppSelector(selectPageTree)
  const pages = useMemo(
    () => [ ...tree.byId.values() ].filter((page) => page.labels.includes(label)),
    [ tree, label ],
  )

  return <Page
    title={label}
    parents={[{ label: t('title'), href: UrlTree.knowledge }]}
  >
    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {status === 'failed' && <EmptyNotice>{t('pages.treeLoadFailed')}</EmptyNotice>}
    {status === 'loaded' && <LabelPagesTable label={label} pages={pages} tree={tree} />}
  </Page>
}

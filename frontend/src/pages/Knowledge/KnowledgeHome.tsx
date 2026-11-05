// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { Navigate, useLocation } from 'react-router'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectPageTree } from '../../store/pagesSlice'

// User interface
import { Spinner } from '@heroui/react'
import { EmptyNotice } from '../../components/EmptyNotice'
import { Page } from '../../layout/page/Page'

// Misc
import { usePagesLoader } from '../../hooks/useServerData'
import { getKnowledgePageUrl, UrlTree } from '../../urls'

// `/knowledge`: the area opens on its first page, beside the tree. An address with a query is a
// search's, which a shared link may still carry, so it goes on to the search with its query.
export function KnowledgeHome() {
  const { t } = useTranslation('knowledge')
  const { search } = useLocation()
  const status = usePagesLoader()
  const tree = useAppSelector(selectPageTree)
  const firstPage = tree.childrenByParent.get(null)?.[0]

  if (search) {
    return <Navigate to={{ pathname: UrlTree.knowledgeSearch, search }} replace />
  }
  if (firstPage) {
    return <Navigate to={getKnowledgePageUrl(firstPage.id)} replace />
  }

  return <Page title={t('title')}>
    {status === 'loading' && <div className='grid place-items-center py-16'>
      <Spinner />
    </div>}
    {status === 'loaded' && <EmptyNotice>{t('pages.none')}</EmptyNotice>}
    {status === 'failed' && <EmptyNotice>{t('pages.treeLoadFailed')}</EmptyNotice>}
  </Page>
}

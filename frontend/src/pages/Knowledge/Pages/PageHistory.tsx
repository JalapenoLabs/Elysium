// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Link } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'
import { PageHistoryList } from './PageHistoryList'
import { PageVersionCompare } from './PageVersionCompare'
import { PageVersionReader } from './PageVersionReader'

// Misc
import { getPageHistoryUrl } from '../../../urls'
import { readPageHistoryView } from './pageHistoryPresentation'

type Props = {
  page: KnowledgePage
}

// How many entries the list shows at first, and how many more each Show older adds.
const SHOWN_ENTRIES_STEP = 30

// A page's History tab: the list, one version read through, or two compared, as the address says, so
// Back leaves a comparison and a link can open one.
export function PageHistory(props: Props) {
  const { t } = useTranslation('knowledge')
  const [ params ] = useSearchParams()
  const [ shownCount, setShownCount ] = useState(SHOWN_ENTRIES_STEP)
  // At most two, the earlier pick first: a third replaces the earlier of them.
  const [ picked, setPicked ] = useState<number[]>([])
  const view = readPageHistoryView(params)

  if (view.kind === 'list') {
    return <PageHistoryList
      page={props.page}
      shownCount={shownCount}
      onShowMore={() => setShownCount((count) => count + SHOWN_ENTRIES_STEP)}
      picked={picked}
      onPick={(version, isPicked) => setPicked((current) => isPicked
        ? [ ...current, version ].slice(-2)
        : current.filter((pickedVersion) => pickedVersion !== version))}
    />
  }

  return <div>
    <Link
      href={getPageHistoryUrl(props.page.id)}
      className='compact inline-flex items-center gap-1 text-sm text-link no-underline hover:underline'
    >
      <LuArrowLeft className='size-4' aria-hidden />
      <span>{t('pages.history.back')}</span>
    </Link>
    {view.kind === 'version'
      ? <PageVersionReader page={props.page} version={view.version} />
      : <PageVersionCompare page={props.page} from={view.from} to={view.to} />}
  </div>
}

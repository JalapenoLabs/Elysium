// Copyright © 2026 Jalapeno Labs

import type { KnowledgeDocument } from '../../api/routes/knowledgeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { buttonVariants, Link } from '@heroui/react'
import { LuBookOpen, LuExternalLink, LuLightbulb } from 'react-icons/lu'

// Misc
import { getLearningUrl, UrlTree } from '../../urls'

type Props = {
  document: KnowledgeDocument
  sourceLabel: string
}

// Where the document's source can be opened: a learning's page, the company doc, or the
// provider's own page for a record, in a new tab. A record with no link of its own has none.
export function KnowledgeOriginLink(props: Props) {
  const { t } = useTranslation('knowledge')
  const { origin, citation } = props.document
  const className = buttonVariants({ variant: 'outline' })

  if (origin.kind === 'learning') {
    return <Link href={getLearningUrl(origin.learningId)} className={className}>
      <LuLightbulb className='size-4' aria-hidden />
      <span>{t('document.openLearning')}</span>
    </Link>
  }
  if (origin.kind === 'companyDoc') {
    return <Link href={UrlTree.settingsCompanyDoc} className={className}>
      <LuBookOpen className='size-4' aria-hidden />
      <span>{t('document.openCompanyDoc')}</span>
    </Link>
  }
  if (!citation.url) {
    return null
  }
  return <Link href={citation.url} target='_blank' rel='noreferrer' className={className}>
    <LuExternalLink className='size-4' aria-hidden />
    <span>{t('document.openSource', { source: props.sourceLabel })}</span>
  </Link>
}

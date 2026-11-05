// Copyright © 2026 Jalapeno Labs

import type { KnowledgePage } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Misc
import { DateText } from '../../../format/DateText'
import { useUserNames } from '../../../hooks/useUserNames'

type Props = {
  page: Pick<KnowledgePage, 'publishedBy' | 'publishedAt' | 'agent'>
}

// Who last edited a page and when, with the agent that acted for them when one did.
export function PageByline(props: Props) {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const userNames = useUserNames()
  const { publishedBy, publishedAt, agent } = props.page
  const name = userNames[publishedBy] ?? t('common:table.someone')

  return <div className='flex flex-wrap items-center gap-x-2 text-sm opacity-70'>
    <span>{
      agent
        ? t('pages.publishedByAgent', { name, agent: agent.name ?? t('pages.agentUnnamed') })
        : t('pages.publishedBy', { name })
    }</span>
    <span aria-hidden>·</span>
    <DateText value={publishedAt} />
  </div>
}

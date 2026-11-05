// Copyright © 2026 Jalapeno Labs

import type { KnowledgePageSummary } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { LuFileText } from 'react-icons/lu'

// Misc
import { getKnowledgePageUrl } from '../../../urls'

type Props = {
  pages: KnowledgePageSummary[]
}

// The pages directly under this one, in their order.
export function ChildPageList(props: Props) {
  const { t } = useTranslation('knowledge')

  return <section aria-labelledby='page-children-heading'>
    <h2 id='page-children-heading' className='compact text-base font-semibold'>{t('pages.childPages')}</h2>
    <ul className='flex flex-col gap-1'>
      {props.pages.map((page) => <li key={page.id}>
        <Link href={getKnowledgePageUrl(page.id)} className='items-start gap-2 text-link'>
          <LuFileText className='mt-0.5 size-4 shrink-0' aria-hidden />
          <span className='min-w-0 break-words'>{page.title}</span>
        </Link>
      </li>)}
    </ul>
  </section>
}

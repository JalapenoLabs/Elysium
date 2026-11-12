// Copyright © 2026 Jalapeno Labs

import type { WorkBatchLink } from '../../api/routes/workQueueRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Link } from '@heroui/react'
import { LuExternalLink } from 'react-icons/lu'

// Misc
import { chainLinkTarget, chainLinkText } from './chainPresentation'

type Props = {
  link: WorkBatchLink
  // The project of the batch whose chain it is, where a message and a finding open.
  projectId: string
}

// One link of a batch's chain by its text, opening where it lives, an issue and a pull request
// in a new tab; plain text for a record the knowledge base has not indexed yet, and for a segment
// of a meeting the reader may not read, named as a private meeting alone.
export function ChainLinkTitle(props: Props) {
  const { t } = useTranslation('workQueue')
  if (props.link.kind === 'record' && props.link.withheld) {
    return <span className='line-clamp-2 italic'>{t('batch.why.privateMeeting')}</span>
  }
  const target = chainLinkTarget(props.link, props.projectId)
  const text = chainLinkText(props.link)
  if (!target) {
    return <span className='line-clamp-2'>{text}</span>
  }
  if (target.isExternal) {
    // HeroUI's link styles set its display, so the row that keeps the icon beside the text's
    // first line, however narrow the column, is a span of its own.
    return <Link href={target.href} target='_blank' rel='noreferrer' className='text-link'>
      <span className='flex items-start gap-1'>
        <span className='line-clamp-2 min-w-0'>{text}</span>
        <LuExternalLink className='mt-1 size-3.5 shrink-0' aria-hidden />
      </span>
    </Link>
  }
  return <Link href={target.href} className='text-link'>
    <span className='line-clamp-2'>{text}</span>
  </Link>
}

// Copyright © 2026 Jalapeno Labs

import type { ElysiaWork } from '../api/routes/elysiaRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'

// Misc
import { workStatusLabelKeys } from './presentation'

type Props = {
  work: ElysiaWork[]
}

// What Elysia has taken on and not finished, with when she expects each to be ready.
export function ElysiaWorkList(props: Props) {
  const { t, i18n } = useTranslation('elysia')
  const readyFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  return <ul className='flex flex-col gap-2'>{
    props.work.map((item) => <li key={item.id} className='rounded-xl bg-surface p-3'>
      <div className='level gap-2'>
        <span className='text-sm font-semibold'>{item.title}</span>
        <Chip size='sm' variant='soft'>{t(workStatusLabelKeys[item.status])}</Chip>
      </div>
      {Boolean(item.detail) && <p className='mt-1 text-sm whitespace-pre-line opacity-80'>{item.detail}</p>}
      {item.readyAt && item.status === 'working' && <p className='mt-1 text-xs opacity-60'>{
        t('work.readyAt', { time: readyFormatter.format(new Date(item.readyAt)) })
      }</p>}
      {Boolean(item.revisits) && <p className='mt-1 text-xs opacity-60'>{
        t('work.revisits', { count: item.revisits })
      }</p>}
    </li>)
  }</ul>
}

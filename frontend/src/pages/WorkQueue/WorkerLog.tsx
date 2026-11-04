// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'
import { selectWorkerLog } from '../../store/workQueueSlice'

// User interface
import { Spinner } from '@heroui/react'
import { LuChevronRight } from 'react-icons/lu'

// Misc
import { useWorkerLogLoader } from '../../hooks/useServerData'

type Props = {
  batchId: string
  // A running batch's log is what people come to read, so it starts open.
  startsOpen: boolean
}

// What a batch's worker said and which tools it called, one line each, the newest 64 KiB. Read
// only while open, and again on every `workBatch.changed`, scrolled to its newest line.
export function WorkerLog(props: Props) {
  const { t } = useTranslation('workQueue')
  const [ isOpen, setIsOpen ] = useState(props.startsOpen)
  const status = useWorkerLogLoader(props.batchId, isOpen)
  const log = useAppSelector((state) => selectWorkerLog(state, props.batchId))
  const listRef = useRef<HTMLOListElement>(null)

  useEffect(() => {
    const list = listRef.current
    if (list) {
      list.scrollTop = list.scrollHeight
    }
  }, [ log, isOpen ])

  const lines = log?.split('\n') ?? []
  if (lines.at(-1) === '') {
    lines.pop()
  }

  let body = <Spinner size='sm' />
  if (log === undefined && status === 'failed') {
    body = <p className='text-sm text-danger'>{t('batch.logError')}</p>
  }
  else if (log !== undefined && !lines.length) {
    body = <p className='text-sm opacity-70'>{t('batch.logEmpty')}</p>
  }
  else if (log !== undefined) {
    body = <ol
      ref={listRef}
      className='max-h-[32rem] overflow-auto rounded-lg bg-surface-secondary py-2 font-mono text-xs'
    >{
        lines.map((line, index) => <li
          // The log is replaced whole on every read, so a line's place serves as its key.
          key={index}
          className='grid grid-cols-[3.5rem_1fr] gap-3 px-2'
        >
          <span className='select-none text-right opacity-50'>{index + 1}</span>
          <span className='whitespace-pre-wrap break-words'>{line}</span>
        </li>)
      }</ol>
  }

  return <section className='relaxed max-w-3xl'>
    <h2 className='compact text-lg font-semibold'>
      <button
        type='button'
        className='level-left cursor-pointer gap-2'
        aria-expanded={isOpen}
        onClick={() => setIsOpen((open) => !open)}
      >
        <LuChevronRight
          className={`size-4 shrink-0 opacity-60 transition-transform motion-reduce:transition-none ${
            isOpen
              ? 'rotate-90'
              : ''
          }`}
          aria-hidden
        />
        <span>{t('batch.log')}</span>
      </button>
    </h2>
    {isOpen && body}
  </section>
}

// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent } from 'react'
import type { FileEntry } from '../proto/generated/storage_pb'

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Button, Tooltip } from '@heroui/react'
import { LuDownload, LuFile, LuFolder, LuPencil, LuTrash2 } from 'react-icons/lu'
import { OhLookAtTheTime } from '../components/OhLookAtTheTime'

// Utility
import { formatBytes } from '@jalapenolabs/uikit'

// The Drive listing (docs/ux.md, keyboard-first): a focusable list where
// arrows move, Enter opens a folder or downloads a file, and every row action
// is also a button. Presentational; the panel owns the data and actions.
type Props = {
  entries: FileEntry[]
  activeIndex: number
  onActiveIndexChange: (index: number) => void
  onOpen: (entry: FileEntry) => void
  onDownload: (entry: FileEntry) => void
  onRename: (entry: FileEntry) => void
  onDelete: (entry: FileEntry) => void
  isBusy?: boolean
}

export function FileList(props: Props) {
  const { t } = useTranslation()

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      props.onActiveIndexChange(Math.min(props.activeIndex + 1, props.entries.length - 1))
    }
    else if (event.key === 'ArrowUp') {
      event.preventDefault()
      props.onActiveIndexChange(Math.max(props.activeIndex - 1, 0))
    }
    else if (event.key === 'Enter') {
      event.preventDefault()
      const entry = props.entries[props.activeIndex]
      if (entry) {
        props.onOpen(entry)
      }
    }
  }

  return <div
    role='listbox'
    tabIndex={0}
    aria-label={t('drive.files.listLabel')}
    className='outline-none'
    onKeyDown={onKeyDown}
  >
    { props.entries.map((entry, index) => (
      <div
        key={entry.path}
        role='option'
        aria-selected={index === props.activeIndex}
        className={
          'level cursor-default items-center rounded-lg px-3 py-2 '
          + (index === props.activeIndex ? 'bg-primary/10' : 'hover:bg-default/30')
        }
        onClick={() => props.onActiveIndexChange(index)}
        onDoubleClick={() => props.onOpen(entry)}
      >
        <div className='flex min-w-0 items-center gap-3'>
          { entry.isDir
            ? <LuFolder className='h-4 w-4 shrink-0 opacity-70' />
            : <LuFile className='h-4 w-4 shrink-0 opacity-50' /> }
          <span className='truncate text-sm'>{ entry.name }</span>
        </div>
        <div className='flex shrink-0 items-center gap-4'>
          <span className='w-20 text-right text-xs opacity-50'>{
            entry.isDir ? '' : formatBytes(Number(entry.size))
          }</span>
          <span className='w-28 text-right text-xs opacity-50'>{
            entry.modified
              ? <OhLookAtTheTime timestamp={entry.modified} relative />
              : null
          }</span>
          <span className='flex items-center gap-1'>
            { !entry.isDir && (
              <Tooltip content={t('drive.files.download')}>
                <div>
                  <Button
                    isIconOnly
                    size='sm'
                    variant='light'
                    aria-label={t('drive.files.download')}
                    isDisabled={props.isBusy}
                    onPress={() => props.onDownload(entry)}
                  >
                    <LuDownload className='h-4 w-4 opacity-60' />
                  </Button>
                </div>
              </Tooltip>
            ) }
            <Tooltip content={t('drive.files.rename')}>
              <div>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  aria-label={t('drive.files.rename')}
                  isDisabled={props.isBusy}
                  onPress={() => props.onRename(entry)}
                >
                  <LuPencil className='h-4 w-4 opacity-60' />
                </Button>
              </div>
            </Tooltip>
            <Tooltip content={t('drive.files.delete')}>
              <div>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  color='danger'
                  aria-label={t('drive.files.delete')}
                  isDisabled={props.isBusy}
                  onPress={() => props.onDelete(entry)}
                >
                  <LuTrash2 className='h-4 w-4 opacity-60' />
                </Button>
              </div>
            </Tooltip>
          </span>
        </div>
      </div>
    )) }
  </div>
}

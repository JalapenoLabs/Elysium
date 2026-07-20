// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input, Tooltip } from '@heroui/react'
import { LuSearch } from 'react-icons/lu'

// Misc
import { openDriveSearch } from '../../drive/searchController'

// The taskbar search: a quiet icon that expands into the universal search bar.
// Enter hands the query to the unified drive search; people/messages/repos
// join it as those pillars land (CLAUDE.md's action-center vision).
export function TaskbarSearch() {
  const { t } = useTranslation('common')

  const [ isExpanded, setIsExpanded ] = useState(false)
  const [ query, setQuery ] = useState('')

  // Focus lands in the field the moment it expands; collapsing clears the
  // query so a stale search never lingers in the bar.
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (isExpanded) {
      inputRef.current?.focus()
    }
    else {
      setQuery('')
    }
  }, [ isExpanded ])

  if (!isExpanded) {
    return <Tooltip content={t('taskbar.search')} placement='top'>
      <div>
        <Button
          isIconOnly
          variant='light'
          aria-label={t('taskbar.search')}
          onPress={() => setIsExpanded(true)}
        >
          <LuSearch className='h-4 w-4' />
        </Button>
      </div>
    </Tooltip>
  }

  return <Input
    ref={inputRef}
    size='sm'
    className='taskbar__search'
    placeholder={t('taskbar.searchPlaceholder')}
    startContent={<LuSearch className='h-4 w-4 opacity-60' />}
    value={query}
    onChange={(event) => setQuery(event.currentTarget.value)}
    onBlur={() => setIsExpanded(false)}
    onKeyDown={(event) => {
      if (event.key === 'Escape') {
        setIsExpanded(false)
      }
      if (event.key === 'Enter') {
        openDriveSearch(query)
        setIsExpanded(false)
      }
    }}
  />
}

// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input, Tooltip } from '@heroui/react'
import { LuSearch } from 'react-icons/lu'

// The taskbar search: a quiet icon that expands into the universal search bar.
// Today it only carries the interaction; results arrive with the universal
// search / ctrl+p navigation work (see CLAUDE.md's action-center vision).
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
        // Universal search lands with the workspace; until then Enter simply
        // closes the bar rather than pretending to search.
        console.debug('universal search is not built yet', { query })
        setIsExpanded(false)
      }
    }}
  />
}

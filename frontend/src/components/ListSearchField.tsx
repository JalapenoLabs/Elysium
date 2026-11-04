// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef } from 'react'

// User interface
import { SearchField } from '@heroui/react'

type Props = {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
}

// Where typing belongs to a field already, so Ctrl+F keeps the browser's find there.
const TEXT_ENTRY_SELECTOR = 'input, textarea, select, [contenteditable="true"]'

// The search for a list shown as more than a table, such as projects as a table or as tiles,
// so it cannot be the table's own. It looks and behaves like the search uikit's tables put in
// the titlebar
// (`~/.claude/docs/ux.md`, Table layout and Search inputs): a magnifying glass, a clear button
// while it holds text, narrow until something is typed and then wider, full width on phones,
// and Ctrl+F (Cmd+F) focuses it unless focus is already in a text field.
export function ListSearchField(props: Props) {
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    function focusOnFind(event: KeyboardEvent) {
      const isFind = (event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'f'
      const target = event.target
      const isInTextEntry = target instanceof Element && Boolean(target.closest(TEXT_ENTRY_SELECTOR))
      if (!isFind || event.defaultPrevented || isInTextEntry) {
        return
      }
      event.preventDefault()
      inputRef.current?.focus()
      inputRef.current?.select()
    }

    window.addEventListener('keydown', focusOnFind)
    return () => window.removeEventListener('keydown', focusOnFind)
  }, [])

  return <SearchField
    id={props.id}
    autoComplete='off'
    aria-label={props.label}
    value={props.value}
    onChange={props.onChange}
    className={[
      'w-full transition-[width] duration-200 motion-reduce:transition-none',
      props.value
        ? 'sm:w-88'
        : 'sm:w-56',
    ].join(' ')}
  >
    <SearchField.Group>
      <SearchField.SearchIcon />
      <SearchField.Input ref={inputRef} className='w-full' placeholder={props.label} />
      <SearchField.ClearButton />
    </SearchField.Group>
  </SearchField>
}

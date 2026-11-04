// Copyright © 2026 Jalapeno Labs

// User interface
import { SearchField } from '@heroui/react'

// Misc
import { useFindHotkey } from '../hooks/useFindHotkey'

type Props = {
  id: string
  label: string
  value: string
  onChange: (value: string) => void
}

// The search for a list shown as more than a table, such as projects as a table or as tiles,
// so it cannot be the table's own. It looks and behaves like the search uikit's tables put in
// the titlebar
// (`~/.claude/docs/ux.md`, Table layout and Search inputs): a magnifying glass, a clear button
// while it holds text, narrow until something is typed and then wider, full width on phones,
// and Ctrl+F (Cmd+F) focuses it unless focus is already in a text field.
export function ListSearchField(props: Props) {
  const inputRef = useFindHotkey()

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

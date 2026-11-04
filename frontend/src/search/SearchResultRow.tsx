// Copyright © 2026 Jalapeno Labs

import type { SearchResult } from './searchTypes'

// User interface
import { Button, Link } from 'react-aria-components'

type Props = {
  result: SearchResult
}

const ROW_CLASS_NAME = [
  'flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left outline-none',
  'transition-colors duration-200 ease-out motion-reduce:transition-none',
  'data-hovered:bg-default data-focus-visible:ring-2 data-focus-visible:ring-focus',
].join(' ')

// One result on the search page: a real link when it goes somewhere, so it opens in a new
// tab on a middle click, and a button when it acts in place. React Aria's unstyled pieces,
// routed through `AppShell`'s `RouterProvider`, which report hover for a real mouse only.
export function SearchResultRow(props: Props) {
  const result = props.result
  const content = <>
    <result.icon className='size-5 shrink-0 opacity-80' aria-hidden />
    <span className='flex min-w-0 flex-col'>
      <span className='truncate font-medium'>{result.title}</span>
      {result.detail && <span className='truncate text-sm opacity-60'>{result.detail}</span>}
    </span>
  </>

  if (result.target.kind === 'link') {
    return <Link href={result.target.href} className={ROW_CLASS_NAME}>{content}</Link>
  }
  return <Button onPress={result.target.run} className={ROW_CLASS_NAME}>{content}</Button>
}

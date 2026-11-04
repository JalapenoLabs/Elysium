// Copyright © 2026 Jalapeno Labs

import type { DateInput } from './dates'

// User interface
import { Tooltip } from '@heroui/react'

// Misc
import { formatDateTime, formatShortDate } from './dates'

type Props = {
  value: DateInput
  className?: string
}

// A date in its short form, "January 1st", for tables and lists, with the full timestamp in a
// tooltip. The tooltip's trigger takes focus, so the full timestamp reaches the keyboard too.
export function DateText(props: Props) {
  const date = new Date(props.value)

  return <Tooltip delay={400}>
    <Tooltip.Trigger className='inline'>
      <time
        dateTime={Number.isNaN(date.getTime())
          ? undefined
          : date.toISOString()}
        className={props.className}
      >{
          formatShortDate(props.value)
        }</time>
    </Tooltip.Trigger>
    <Tooltip.Content>{
      formatDateTime(props.value)
    }</Tooltip.Content>
  </Tooltip>
}

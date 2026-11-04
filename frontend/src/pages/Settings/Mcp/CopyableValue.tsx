// Copyright © 2026 Jalapeno Labs

// User interface
import { CopyButton } from '../../../components/CopyButton'

type Props = {
  label: string
  value: string
  // Names what the button copies, such as "Copy command".
  copyLabel: string
}

// One value to paste somewhere else, such as a command, with a button that copies it.
export function CopyableValue(props: Props) {
  return <div className='compact'>
    <p className='mb-1 text-xs font-medium opacity-70'>{props.label}</p>
    <div className='level gap-2 rounded-lg bg-surface-secondary px-3 py-2'>
      <code className='min-w-0 flex-1 overflow-x-auto whitespace-nowrap text-sm'>{props.value}</code>
      <CopyButton value={props.value} label={props.copyLabel} />
    </div>
  </div>
}

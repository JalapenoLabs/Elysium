// Copyright © 2026 Jalapeno Labs

// User interface
import { Chip, Link } from '@heroui/react'

// Misc
import { getKnowledgeLabelUrl } from '../../../urls'

type Props = {
  label: string
}

// A label wherever it is read, opening every page carrying it.
export function LabelLink(props: Props) {
  return <Link href={getKnowledgeLabelUrl(props.label)} className='rounded-full no-underline hover:opacity-80'>
    <Chip size='sm' variant='soft'>{props.label}</Chip>
  </Link>
}

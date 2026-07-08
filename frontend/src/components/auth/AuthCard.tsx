// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// UI
import { Card, CardBody } from '@heroui/react'

type Props = {
  children: ReactNode
}

// A translucent, themed surface that floats over the ambient field. Every auth
// screen puts its content in one of these so text stays legible in both themes.
export function AuthCard(props: Props) {
  return <Card className='w-full border border-default-200/40 bg-content1/85 shadow-2xl backdrop-blur-md'>
    <CardBody className='p-6 sm:p-8'>
      { props.children }
    </CardBody>
  </Card>
}

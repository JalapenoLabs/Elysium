// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { FiExternalLink } from 'react-icons/fi'
import { StatusIndicator } from '../components/status/StatusIndicator'

// The embedded-panel chrome (docs/workspace.md): hosts a proxied service UI or
// an external linked view inside a dockable tab. The chrome carries the status
// grammar (active while the page loads, done once it renders) and an
// open-in-browser escape hatch; the rc-dock tab supplies title, close, drag,
// float, and pop-out. Authentication is not this component's job: M3's
// forward-auth/SSO delivers pre-authenticated URLs and cookies, so the page
// simply arrives signed in.
type Props = {
  title: string
  url: string
}

export function EmbeddedPanel(props: Props) {
  const { t } = useTranslation()
  const [ hasLoaded, setHasLoaded ] = useState(false)

  return <div className='flex h-full w-full flex-col'>
    <div className='flex items-center gap-2 border-b border-foreground/10 px-3 py-1.5'>
      <StatusIndicator
        size='sm'
        kind={hasLoaded ? 'done' : 'active'}
      />
      <span className='min-w-0 flex-1 truncate text-xs opacity-60'>{
        props.url
      }</span>
      <Tooltip content={t('workspace.openInBrowser')}>
        <div>
          <Button
            isIconOnly
            size='sm'
            variant='light'
            aria-label={t('workspace.openInBrowser')}
            onPress={() => {
              window.open(props.url, '_blank', 'noopener,noreferrer')
            }}
          >
            <FiExternalLink className='h-4 w-4 opacity-60' />
          </Button>
        </div>
      </Tooltip>
    </div>
    <iframe
      src={props.url}
      title={props.title}
      className='w-full flex-1 border-0 bg-background'
      onLoad={() => setHasLoaded(true)}
    />
  </div>
}

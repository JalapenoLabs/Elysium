// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { ConnectionInstance, ConnectionServer } from '../../connection/bridge'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Card, CardBody, Input } from '@heroui/react'
import { LuServer, LuTrash2 } from 'react-icons/lu'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'

// The picker the desktop shell shows before login when no instance is selected:
// connect to any self-hosted Elysium by IP or domain. Testing previews whose
// instance it is before saving; connecting saves and switches the client to it
// (the shell clears cookies and reloads). Saved instances are listed for
// one-click switching and removal. Browser builds never see this (same-origin).
export function ConnectionScreen() {
  const { t } = useTranslation([ 'auth', 'common' ])

  // Present here by construction: the gate only renders this in the desktop shell.
  const connection = window.elysium?.connection

  const [ servers, setServers ] = useState<ConnectionServer[]>([])
  const [ url, setUrl ] = useState('')
  const [ tested, setTested ] = useState<ConnectionInstance | null>(null)
  const [ isTesting, setIsTesting ] = useState(false)
  const [ isConnecting, setIsConnecting ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function loadServers() {
    if (!connection) {
      return
    }
    try {
      setServers(await connection.list())
    }
    catch (error) {
      console.debug('could not list saved connections', { error })
    }
  }

  useEffect(() => {
    loadServers()
  }, [])

  // Any edit to the URL invalidates a prior test result.
  function onUrlChange(value: string) {
    setUrl(value)
    setTested(null)
    setErrorMessage(null)
  }

  async function onTest(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!connection || !url.trim()) {
      return
    }
    setIsTesting(true)
    setErrorMessage(null)
    setTested(null)
    try {
      setTested(await connection.test(url.trim()))
    }
    catch (error) {
      console.debug('connection test failed', { error })
      setErrorMessage(t('auth:connection.testFailed'))
    }
    finally {
      setIsTesting(false)
    }
  }

  // Saves the tested instance and switches to it. `select` reloads the window,
  // so there's nothing to do after it resolves.
  async function onConnect() {
    if (!connection || !url.trim()) {
      return
    }
    setIsConnecting(true)
    setErrorMessage(null)
    try {
      const { server } = await connection.add(url.trim())
      await connection.select(server.id)
    }
    catch (error) {
      console.debug('connect failed', { error })
      setErrorMessage(t('auth:connection.connectFailed'))
      setIsConnecting(false)
    }
  }

  async function onSelect(server: ConnectionServer) {
    if (!connection) {
      return
    }
    // Reloads the window on success; no follow-up needed.
    await connection.select(server.id)
  }

  async function onRemove(server: ConnectionServer) {
    if (!connection) {
      return
    }
    try {
      await connection.remove(server.id)
      await loadServers()
    }
    catch (error) {
      console.debug('could not remove connection', { error })
    }
  }

  return <AuthShell state='idle' showPresence={false}>
    <AuthCard>
      <div className='flex items-center gap-2'>
        <LuServer className='h-5 w-5' />
        <p className='text-xs uppercase tracking-widest opacity-60'>
          { t('auth:connection.eyebrow') }
        </p>
      </div>
      <h1 className='mt-2 text-2xl'>
        <strong>{ t('auth:connection.title') }</strong>
      </h1>
      <p className='mt-3 opacity-80'>{ t('auth:connection.subtitle') }</p>

      <form onSubmit={onTest}>
        <div className='mt-6 compact'>
          <Input
            autoFocus
            label={t('auth:connection.urlLabel')}
            placeholder='https://elysium.example.com'
            value={url}
            onChange={(event) => onUrlChange(event.currentTarget.value)}
          />
        </div>

        { tested
          ? <Card className='compact bg-success/10'>
            <CardBody>
              <p className='text-sm'>
                { t('auth:connection.previewName', {
                  name: tested.name || t('auth:connection.unnamed'),
                }) }
              </p>
              <p className='text-xs opacity-60'>
                { t('auth:connection.previewMeta', {
                  version: tested.version,
                  state: tested.claimed
                    ? t('auth:connection.claimed')
                    : t('auth:connection.unclaimed'),
                }) }
              </p>
            </CardBody>
          </Card>
          : null }

        { errorMessage
          ? <p className='compact text-sm text-danger'>{ errorMessage }</p>
          : null }

        <div className='mt-6 level'>
          <Button
            type='submit'
            variant='flat'
            isLoading={isTesting}
            isDisabled={!url.trim() || isTesting}
          >
            <span>{ t('auth:connection.test') }</span>
          </Button>
          <Button
            color='primary'
            isLoading={isConnecting}
            isDisabled={!tested || isConnecting}
            onPress={onConnect}
          >
            <span>{ t('auth:connection.connect') }</span>
          </Button>
        </div>
      </form>

      { servers.length
        ? <div className='mt-8'>
          <p className='text-sm opacity-60'>{ t('auth:connection.saved') }</p>
          <div className='mt-2 flex flex-col gap-2'>
            { servers.map((server) =>
              <div
                key={server.id}
                className='level items-center rounded-medium bg-default-100 px-3 py-2'
              >
                <button
                  type='button'
                  className='min-w-0 text-left'
                  onClick={() => onSelect(server)}
                >
                  <p className='truncate text-sm'>{ server.name || server.url }</p>
                  <p className='truncate text-xs opacity-60'>{ server.url }</p>
                </button>
                <Button
                  isIconOnly
                  size='sm'
                  variant='light'
                  aria-label={t('auth:connection.remove')}
                  onPress={() => onRemove(server)}
                >
                  <LuTrash2 className='h-4 w-4' />
                </Button>
              </div>,
            ) }
          </div>
        </div>
        : null }
    </AuthCard>
  </AuthShell>
}

// Copyright © 2026 Jalapeno Labs

import type { MeResponse } from '../../proto/generated/auth_pb'

// Core
import { useState } from 'react'

// UI
import { Button, Card, CardBody, CardHeader, Code, Divider } from '@heroui/react'

// Misc
import { useSession } from '../../auth/useSession'
import { getMe } from '../../api/routes/authRoutes'

// Exercises the protected `/api/v1/me` route with the session cookie. When signed
// out it invites sign-in; when signed in it loads and shows the principal.
export function ProfilePanel() {
  const session = useSession()
  const [ me, setMe ] = useState<MeResponse | null>(null)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)
  const [ isLoading, setIsLoading ] = useState(false)

  async function loadProfile() {
    setIsLoading(true)
    setErrorMessage(null)

    try {
      const response = await getMe()
      setMe(response)
    }
    catch (error) {
      console.debug('ProfilePanel failed to load /me', error)
      setMe(null)
      setErrorMessage('Could not load your profile. Are you signed in?')
    }
    finally {
      setIsLoading(false)
    }
  }

  return <Card className='p-2'>
    <CardHeader>
      <h2 className='text-xl'>
        <strong>Your profile</strong>
      </h2>
    </CardHeader>
    <Divider />
    <CardBody className='relaxed'>
      <p className='opacity-80'>
        Calls the protected /api/v1/me route with your session cookie.
      </p>{
        session.isAuthenticated
          ? <div className='level'>
            <Button
              color='primary'
              isLoading={isLoading}
              isDisabled={isLoading}
              onPress={loadProfile}
            >
              <span>Load my profile</span>
            </Button>
          </div>
          : <p className='opacity-80'>Sign in from the navigation bar to load your profile.</p>
      }{
        me
          && <div className='compact'>
            <Code color='success'>{ `${me.username || me.subject} · ${me.email || 'no email'}` }</Code>
          </div>
      }{
        errorMessage
          && <Code color='danger'>{ errorMessage }</Code>
      }
    </CardBody>
  </Card>
}

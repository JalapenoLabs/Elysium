// Core
import { useState } from 'react'

// UI
import { Button, Card, CardBody, CardHeader, Chip, Code, Divider } from '@heroui/react'

// Redux
import { useAppDispatch, useAppSelector } from '../../store/hooks'
import { increment, decrement, reset } from '../../store/counterSlice'

// Misc
import { getHello, getDatabaseHealth } from '../../api/routes/healthRoutes'

export function Home() {
  const dispatch = useAppDispatch()
  const count = useAppSelector((state) => state.counter.value)

  const [ apiMessage, setApiMessage ] = useState<string | null>(null)
  const [ databaseStatus, setDatabaseStatus ] = useState<string | null>(null)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)
  const [ isLoading, setIsLoading ] = useState(false)

  async function pingApi() {
    setIsLoading(true)
    setErrorMessage(null)

    try {
      const hello = await getHello()
      const databaseHealth = await getDatabaseHealth()
      setApiMessage(hello.message)
      setDatabaseStatus(databaseHealth.reachable ? 'reachable' : 'unreachable')
    }
    catch (error) {
      console.debug('Home failed to reach the API', { error })
      setApiMessage(null)
      setDatabaseStatus(null)
      setErrorMessage('Could not reach the API. Is the backend running on port 31416?')
    }
    finally {
      setIsLoading(false)
    }
  }

  return <section className='container mx-auto p-6'>
    <div className='relaxed'>
      <h1 className='text-3xl'>
        <strong>Welcome to Elysia</strong>
      </h1>
      <p className='opacity-80'>
        A base template wiring a Rust API and a Postgres database to this React SPA.
      </p>
    </div>

    <div className='relaxed'>
      <Card className='p-2'>
        <CardHeader>
          <h2 className='text-xl'>
            <strong>Redux counter</strong>
          </h2>
        </CardHeader>
        <Divider />
        <CardBody className='relaxed'>
          <p className='opacity-80'>
            State lives in a Redux Toolkit slice, proving the store is wired up.
          </p>
          <div className='level'>
            <Chip size='lg' variant='flat' color='secondary'>
              { count }
            </Chip>
            <div className='level-right'>
              <Button variant='flat' onPress={() => dispatch(decrement())}>
                <span>Decrement</span>
              </Button>
              <Button variant='flat' onPress={() => dispatch(reset())}>
                <span>Reset</span>
              </Button>
              <Button color='primary' onPress={() => dispatch(increment())}>
                <span>Increment</span>
              </Button>
            </div>
          </div>
        </CardBody>
      </Card>
    </div>

    <div className='relaxed'>
      <Card className='p-2'>
        <CardHeader>
          <h2 className='text-xl'>
            <strong>API connectivity</strong>
          </h2>
        </CardHeader>
        <Divider />
        <CardBody className='relaxed'>
          <p className='opacity-80'>
            Calls the Rust API with Ky and checks that it can reach Postgres.
          </p>
          <div className='level'>
            <Button
              color='primary'
              isLoading={isLoading}
              isDisabled={isLoading}
              onPress={pingApi}
            >
              <span>Ping the API</span>
            </Button>
          </div>
          { apiMessage &&
            <div className='compact'>
              <Code color='success'>{ apiMessage }</Code>
            </div>
          }
          { databaseStatus &&
            <div className='level'>
              <span className='opacity-80'>Database</span>
              <Chip
                variant='flat'
                color={databaseStatus === 'reachable' ? 'success' : 'danger'}
              >
                { databaseStatus }
              </Chip>
            </div>
          }
          { errorMessage &&
            <Code color='danger'>{ errorMessage }</Code>
          }
        </CardBody>
      </Card>
    </div>
  </section>
}

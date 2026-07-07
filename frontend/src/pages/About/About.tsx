// UI
import { Card, CardBody, CardHeader, Chip, Divider, Link } from '@heroui/react'

// Misc
import { ExternalLinks } from '../../urls'

const stack = [
  'React 19',
  'Redux Toolkit',
  'React Router',
  'HeroUI v2',
  'Tailwind CSS',
  'Ky',
  'Vite',
  'Rust + Axum',
  'Postgres',
] as const

export function About() {
  return <section className='container mx-auto p-6'>
    <div className='relaxed'>
      <h1 className='text-3xl'>
        <strong>About this template</strong>
      </h1>
      <p className='opacity-80'>
        A client-side rendered single-page app, hot-reloaded end to end via Docker Compose.
      </p>
    </div>

    <div className='relaxed'>
      <Card className='p-2'>
        <CardHeader>
          <h2 className='text-xl'>
            <strong>The stack</strong>
          </h2>
        </CardHeader>
        <Divider />
        <CardBody className='relaxed'>
          <div className='flex flex-wrap gap-2'>
            { stack.map((item) =>
              <Chip key={item} variant='flat'>{ item }</Chip>,
            ) }
          </div>
          <div className='level-right'>
            <Link isExternal showAnchorIcon href={ExternalLinks.heroui}>HeroUI</Link>
            <Link isExternal showAnchorIcon href={ExternalLinks.vite}>Vite</Link>
            <Link isExternal showAnchorIcon href={ExternalLinks.reactRouter}>React Router</Link>
          </div>
        </CardBody>
      </Card>
    </div>
  </section>
}

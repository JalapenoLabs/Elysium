// Copyright © 2026 Jalapeno Labs

import type { ProjectCoverFit } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

type Props = {
  // The image to show, or null for the placeholder.
  src: string | null
  fit: ProjectCoverFit
  // The project's name, for the image's description and the placeholder's initial.
  name: string
  // Size, shape, and corner rounding: a 16:9 tile or thumbnail, or a wide page banner.
  className: string
}

// A cover frame, usually 16:9. With `fill` the image covers the frame, cropped to it. With
// `fit` it is contained, never cropped or stretched: a square logo sits centered at full
// height, and a blurred, enlarged copy of the same image fills the space it leaves, so the
// frame reads as one picture rather than a logo on a bar. The placeholder's
// initial and the blur are sized against the frame's width, so a thumbnail looks like a small tile.
export function ProjectCover(props: Props) {
  const { t } = useTranslation('projects')
  const frameClassName = `@container relative overflow-hidden bg-surface-secondary ${props.className}`

  if (!props.src) {
    return <div className={`${frameClassName} grid place-items-center`} aria-hidden>
      <span className='text-[25cqw] leading-none font-semibold uppercase opacity-20'>{
        props.name.trim().charAt(0)
      }</span>
    </div>
  }

  if (props.fit === 'fill') {
    return <div className={frameClassName}>
      <img
        src={props.src}
        alt={t('tile.coverAlt', { name: props.name })}
        className='size-full object-cover'
      />
    </div>
  }

  return <div className={frameClassName}>
    <img
      src={props.src}
      alt=''
      aria-hidden
      className='absolute inset-0 size-full scale-125 object-cover opacity-50 blur-[8cqw]'
    />
    <img
      src={props.src}
      alt={t('tile.coverAlt', { name: props.name })}
      className='relative size-full object-contain'
    />
  </div>
}

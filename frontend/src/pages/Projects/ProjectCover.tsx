// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

type Props = {
  // The image to show, or null for the placeholder.
  src: string | null
  // The project's name, for the image's description and the placeholder's initial.
  name: string
  className?: string
}

// A 16:9 banner. The image is contained, never cropped or stretched: a square logo sits
// centered at full height. A blurred, enlarged copy of the same image fills the space it
// leaves, so the frame reads as one picture rather than a logo on a bar.
export function ProjectCover(props: Props) {
  const { t } = useTranslation('projects')
  const frameClassName = `relative aspect-video w-full overflow-hidden rounded-2xl bg-surface-secondary ${
    props.className ?? ''
  }`

  if (!props.src) {
    return <div className={`${frameClassName} grid place-items-center`} aria-hidden>
      <span className='text-5xl font-semibold uppercase opacity-20'>{
        props.name.trim().charAt(0)
      }</span>
    </div>
  }

  return <div className={frameClassName}>
    <img
      src={props.src}
      alt=''
      aria-hidden
      className='absolute inset-0 size-full scale-110 object-cover opacity-50 blur-2xl'
    />
    <img
      src={props.src}
      alt={t('tile.coverAlt', { name: props.name })}
      className='relative size-full object-contain'
    />
  </div>
}

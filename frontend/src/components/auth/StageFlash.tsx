// Copyright © 2026 Jalapeno Labs

// UI
import { AnimatePresence, motion } from 'framer-motion'

export type StageFlashMessage = {
  // warning: a local validation miss, flashed briefly in yellow.
  // danger: a real failure from the server, shown in red until the next attempt.
  tone: 'warning' | 'danger'
  message: string
  // Unique per flash so an identical repeated message still replays its entrance.
  nonce: number
}

type Props = {
  flash: StageFlashMessage | null
}

const colorByTone = {
  warning: 'text-warning',
  danger: 'text-danger',
} as const satisfies Record<StageFlashMessage['tone'], string>

// The status line under the login gate's hero glyphs. Height is reserved even
// while empty so appearing messages never shove the stage around.
export function StageFlash(props: Props) {
  return <div className='stage-flash' aria-live='polite'>
    <AnimatePresence mode='wait'>
      { props.flash
        ? <motion.p
          key={props.flash.nonce}
          className={colorByTone[props.flash.tone]}
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.25, ease: 'easeOut' }}
        >
          { props.flash.message }
        </motion.p>
        : null }
    </AnimatePresence>
  </div>
}

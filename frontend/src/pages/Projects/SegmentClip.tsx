// Copyright © 2026 Jalapeno Labs

// Misc
import { getTimelineMediaUrl } from '../../api/routes/timelineRoutes'
import { isVideoClip } from './timelinePresentation'

type Props = {
  recordId: string
  contentType: string
}

// A meeting segment's clip, streamed from Elysium's own origin. It loads only once played,
// so a long timeline never downloads every clip on it.
export function SegmentClip(props: Props) {
  const source = getTimelineMediaUrl(props.recordId)

  if (isVideoClip(props.contentType)) {
    return <video controls preload='none' className='w-full max-w-2xl rounded-lg bg-black'>
      <source src={source} type={props.contentType} />
    </video>
  }
  return <audio controls preload='none' className='w-full max-w-2xl'>
    <source src={source} type={props.contentType} />
  </audio>
}

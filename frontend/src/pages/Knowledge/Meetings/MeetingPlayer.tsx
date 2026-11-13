// Copyright © 2026 Jalapeno Labs

import type { MeetingPlayerState } from './useMeetingPlayer'

// User interface
import { PlayerFrame } from './PlayerFrame'

type Props = {
  player: MeetingPlayerState
  // Pins the picture the video shows now, while it shows one and the meeting takes changes.
  onPinFrame: (() => void) | null
}

// A meeting's player along the top of its page (docs/meeting-review.md, The player), while it has a
// recording to play. A meeting in words alone, such as a Slack huddle, has none, and one still
// being fetched gains it as its recording is stored (`meeting.changed` reads the captures again),
// so the frame mounts then, with everything it watches already in place.
export function MeetingPlayer(props: Props) {
  const { capture, src } = props.player
  if (!capture || !src) {
    return null
  }
  return <PlayerFrame
    player={props.player}
    capture={capture}
    src={src}
    onPinFrame={props.onPinFrame}
  />
}

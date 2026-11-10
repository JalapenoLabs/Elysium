# Copyright © 2026 Jalapeno Labs

"""Transcribes one recording with faster-whisper, word by word, for Elysium's meetings.

The engine runs this in a container of its own with no network (api/src/whisper/), so the
contract is the process's standard streams alone:

- the recording arrives on standard input, in any format the bundled FFmpeg reads, video
  included;
- the transcript leaves on standard output as one JSON document, and nothing else is ever
  written there;
- progress goes to standard error, one line at a time, and never quotes what was said,
  since every company member can read the step's log.

It may be told names and terms to listen for (`--hint-words`, one comma-separated argument): the
company's Transcription doc and the names its company doc spells, so a name is heard right more
often before anything corrects it.

It exits 0 with a transcript, NO_AUDIO when the recording has no audio stream, and
UNREADABLE when it cannot be decoded at all. Anything else is a failure another attempt may
get past. The model is read from the directory baked into the image, never fetched.
"""

from __future__ import annotations

import argparse
import logging
import os
import shutil
import sys
import tempfile
import time
from importlib.metadata import version
from pathlib import Path

import av
from faster_whisper import WhisperModel
from pydantic import BaseModel, ConfigDict
from pydantic.alias_generators import to_camel

logger = logging.getLogger(__name__)

# The engine reads these exit codes (api/src/whisper/mod.rs); change them together.
NO_AUDIO = 3
UNREADABLE = 2

# Where the Dockerfile puts the pinned model, and what it names it.
MODEL_DIRECTORY = Path(os.environ["WHISPER_MODEL_DIRECTORY"])
MODEL_NAME = os.environ["WHISPER_MODEL_NAME"]

# How often progress is reported, in seconds of audio transcribed: often enough that a step's
# log shows it moving, rarely enough that an hour's log stays a screenful.
PROGRESS_EVERY_SECONDS = 300.0


# ========================================================================================
# The transcript, as the engine stores it
# ========================================================================================


class _CamelModel(BaseModel):
    """Serializes field names in camelCase, as the engine's JSON is."""

    model_config = ConfigDict(alias_generator=to_camel, populate_by_name=True)


class TranscribedWord(_CamelModel):
    """One word, exactly as Whisper wrote it: its leading space is kept, so a segment's
    words joined with nothing between them read as the segment."""

    start_ms: int
    end_ms: int
    text: str
    probability: float


class TranscribedSegment(_CamelModel):
    """One stretch of speech Whisper decoded together, with its words."""

    start_ms: int
    end_ms: int
    text: str
    words: list[TranscribedWord]


class Transcription(_CamelModel):
    """Everything the engine stores of one recording's transcription. Every time is in whole
    milliseconds from the recording's start, as every transcript in Elysium is timed."""

    model: str
    language: str
    language_probability: float
    duration_ms: int
    segments: list[TranscribedSegment]


# ========================================================================================
# Transcribing
# ========================================================================================


def _clock(seconds: float) -> str:
    """Seconds as `h:mm:ss`, for progress lines."""
    whole = int(seconds)
    return f"{whole // 3600}:{whole % 3600 // 60:02}:{whole % 60:02}"


def _spool_standard_input(directory: Path) -> Path:
    """Copies the recording from standard input into a file, since a recording's index may sit
    at its end (MP4 without `faststart`) and decoding it then needs to seek."""
    spooled = directory / "recording"
    with spooled.open("wb") as recording:
        shutil.copyfileobj(sys.stdin.buffer, recording, length=1024 * 1024)
    return spooled


def transcribe(recording: Path, threads: int, hint_words: str) -> Transcription:
    """Transcribes `recording` on `threads` CPU threads, listening for `hint_words`, and reports
    progress as it goes."""
    started = time.monotonic()
    model = WhisperModel(
        str(MODEL_DIRECTORY),
        device="cpu",
        compute_type="int8",
        cpu_threads=threads,
        num_workers=1,
        local_files_only=True,
    )
    logger.info(f"Loaded {MODEL_NAME} in {time.monotonic() - started:.1f} s, on {threads} threads")

    # The voice activity filter skips silence before decoding, which is where Whisper
    # otherwise invents text; word timestamps are what clips are cut by. Hint words reach the
    # model as the start of its context, where it reads the spellings it then favours. Every
    # other option is faster-whisper's default.
    if hint_words:
        logger.info(f"Listening for {len(hint_words.split(', '))} names and terms")
    segments, info = model.transcribe(
        str(recording),
        word_timestamps=True,
        vad_filter=True,
        hotwords=hint_words or None,
    )
    logger.info(
        f"The recording runs {_clock(info.duration)}; speech is in {info.language} "
        f"(probability {info.language_probability:.2f})"
    )

    transcribed: list[TranscribedSegment] = []
    next_report = PROGRESS_EVERY_SECONDS
    for segment in segments:
        words = [
            TranscribedWord(
                start_ms=round(word.start * 1000),
                end_ms=round(word.end * 1000),
                text=word.word,
                probability=word.probability,
            )
            for word in segment.words or []
        ]
        transcribed.append(
            TranscribedSegment(
                start_ms=round(segment.start * 1000),
                end_ms=round(segment.end * 1000),
                text=segment.text.strip(),
                words=words,
            )
        )
        if segment.end >= next_report:
            logger.info(f"Transcribed {_clock(segment.end)} of {_clock(info.duration)}")
            next_report = segment.end + PROGRESS_EVERY_SECONDS

    logger.info(
        f"Transcribed {len(transcribed)} segments in {time.monotonic() - started:.1f} s"
    )
    return Transcription(
        model=f"{MODEL_NAME}, faster-whisper {version('faster-whisper')}, int8",
        language=info.language,
        language_probability=info.language_probability,
        duration_ms=round(info.duration * 1000),
        segments=transcribed,
    )


def main() -> int:
    """Reads the recording, transcribes it, and writes the transcript to standard output."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--threads", type=int, required=True, help="CPU threads to decode on")
    parser.add_argument(
        "--hint-words",
        default="",
        help="Names and terms to listen for, separated by commas",
    )
    arguments = parser.parse_args()

    # Standard output carries the transcript alone. Libraries that print, from Python or
    # from native code, write to descriptor 1, so it is pointed at standard error for the
    # whole run, and the transcript is written to a copy of the original kept aside.
    transcript_output = os.fdopen(os.dup(sys.stdout.fileno()), "w", encoding="utf-8")
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    logging.basicConfig(level=logging.INFO, format="%(message)s", stream=sys.stderr)
    # faster-whisper's own lines repeat what this script reports.
    logging.getLogger("faster_whisper").setLevel(logging.WARNING)

    with tempfile.TemporaryDirectory() as scratch:
        recording = _spool_standard_input(Path(scratch))
        size = recording.stat().st_size
        logger.info(f"Received a recording of {size / 1_000_000:.1f} MB")
        try:
            with av.open(str(recording), metadata_errors="ignore") as container:
                has_audio = bool(container.streams.audio)
        except av.FFmpegError as error:
            logger.error(f"The recording cannot be read: {error}")
            return UNREADABLE
        if not has_audio:
            logger.warning("The recording has no audio stream, so there is nothing to transcribe")
            return NO_AUDIO

        transcription = transcribe(recording, arguments.threads, arguments.hint_words)

    transcript_output.write(transcription.model_dump_json(by_alias=True))
    transcript_output.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())

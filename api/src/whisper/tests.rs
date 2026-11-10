// Copyright © 2026 Jalapeno Labs

//! Whisper's policy, its container, and the streams it is driven over; and, opt-in, a real
//! transcription through a Docker proxy.

use std::sync::Mutex;

use futures_util::stream;
use image::ImageEncoder as _;
use serde_json::json;
use tokio::io::AsyncReadExt;

use super::*;

#[test]
fn a_transcription_takes_half_the_hosts_cpus_up_to_sixteen() {
    for (cpus, threads) in [(0, 1), (1, 1), (2, 1), (4, 2), (8, 4), (32, 16), (64, 16)] {
        assert_eq!(transcription_threads(cpus), threads, "{cpus} CPUs");
    }
}

#[test]
fn the_container_has_no_network_and_is_capped_at_its_threads() {
    let spec = container::spec("elysium-whisper:dev", 16, &[]);
    assert_eq!(
        spec.cmd.as_deref(),
        Some(&["--threads".to_owned(), "16".to_owned()][..])
    );
    // Names to listen for go as one argument, joined as Whisper reads them.
    let hinted = container::spec(
        "elysium-whisper:dev",
        16,
        &["Sharath".to_owned(), "Elysium".to_owned()],
    );
    assert_eq!(
        hinted.cmd.as_deref(),
        Some(
            &[
                "--threads".to_owned(),
                "16".to_owned(),
                "--hint-words".to_owned(),
                "Sharath, Elysium".to_owned(),
            ][..]
        )
    );
    // The recording arrives on standard input, closed when the attach closes its end.
    assert_eq!(
        (spec.open_stdin, spec.stdin_once, spec.attach_stdin),
        (Some(true), Some(true), Some(true))
    );

    let host = spec.host_config.expect("host config");
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(host.nano_cpus, Some(16_000_000_000));
    assert!(host.cpu_shares.is_some_and(|shares| shares < 1024));
    assert_eq!(
        host.memory, host.memory_swap,
        "no swap on top of its memory"
    );
    assert_eq!(host.cap_drop.as_deref(), Some(&["ALL".to_owned()][..]));
    // Nothing shared: no mounts, no published ports, no privileges.
    assert!(host.binds.is_none() && host.mounts.is_none());
    assert!(host.port_bindings.is_none());
    assert_eq!(host.privileged, None);
}

#[test]
fn a_transcriptions_container_is_named_for_its_step() {
    assert_eq!(
        container::name(Uuid::nil()),
        "elysium-whisper-00000000-0000-0000-0000-000000000000"
    );
}

#[test]
fn the_transcript_reads_as_the_transcriber_writes_it() {
    // What whisper/transcribe.py prints, field for field.
    let printed = json!({
        "model": "Whisper large-v3 (Systran/faster-whisper-large-v3@edaa852e), faster-whisper 1.2.1, int8",
        "language": "en",
        "languageProbability": 0.99,
        "durationMs": 30000,
        "segments": [{
            "startMs": 820,
            "endMs": 2600,
            "text": "This is a recording.",
            "words": [
                { "startMs": 820, "endMs": 1100, "text": " This", "probability": 0.98 },
                { "startMs": 1100, "endMs": 1300, "text": " is", "probability": 0.99 },
            ],
        }],
    });
    let transcript: WhisperTranscript =
        serde_json::from_value(printed.clone()).expect("the transcript reads");
    assert_eq!(transcript.duration_ms, 30_000);
    assert_eq!(transcript.segments[0].words[1].text, " is");
    // Stored as it was read, so a later step reads the same document.
    assert_eq!(
        serde_json::to_value(&transcript).expect("serializes"),
        printed
    );
}

#[test]
fn only_what_another_attempt_may_pass_is_retried() {
    let exited = WhisperError::Exited {
        status: 1,
        output: "killed".to_owned(),
    };
    assert!(exited.is_retryable());
    assert!(WhisperError::Recording("storage stalled".to_owned()).is_retryable());
    for permanent in [
        WhisperError::Unreadable("Invalid data".to_owned()),
        WhisperError::OutOfMemory,
        WhisperError::TimedOut,
        WhisperError::TooLarge,
    ] {
        assert!(!permanent.is_retryable(), "{permanent}");
    }
    assert_eq!(
        WhisperError::OutOfMemory.to_string(),
        "Whisper ran out of its 12 GiB of memory"
    );
}

#[tokio::test]
async fn the_recording_streams_in_whole_and_its_end_closes_the_input() {
    let (writer, mut reader) = tokio::io::duplex(16);
    let recording = stream::iter([
        Ok::<_, std::io::Error>(Bytes::from_static(b"first ")),
        Ok(Bytes::from_static(b"and the rest")),
    ]);
    let reading = async {
        let mut received = Vec::new();
        reader.read_to_end(&mut received).await.expect("reads");
        received
    };
    let (fed, received) = tokio::join!(feed(Box::pin(writer), recording), reading);
    fed.expect("fed");
    // `read_to_end` returning at all is the input having been closed.
    assert_eq!(received, b"first and the rest");
}

#[tokio::test]
async fn a_recording_that_fails_part_way_fails_the_transcription() {
    let (writer, _reader) = tokio::io::duplex(1024);
    let recording = stream::iter([
        Ok(Bytes::from_static(b"first")),
        Err("the provider stalled"),
    ]);
    let fed = feed(Box::pin(writer), recording).await;
    assert!(
        matches!(&fed, Err(WhisperError::Recording(reason)) if reason == "the provider stalled"),
        "{fed:?}"
    );
}

#[tokio::test]
async fn a_container_that_stops_reading_is_left_to_its_exit_status() {
    let (writer, reader) = tokio::io::duplex(4);
    drop(reader);
    let recording = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
        b"more than four",
    ))]);
    feed(Box::pin(writer), recording)
        .await
        .expect("the exit status says why, not the write");
}

#[tokio::test]
async fn standard_output_is_the_transcript_and_each_line_of_standard_error_is_progress() {
    let chunks = vec![
        Ok(LogOutput::StdErr {
            message: Bytes::from_static(b"Loaded the model\nTransc"),
        }),
        Ok(LogOutput::StdOut {
            message: Bytes::from_static(b"{\"model\":"),
        }),
        Ok(LogOutput::StdErr {
            message: Bytes::from_static(b"ribed 0:05:00 of 0:18:14\n"),
        }),
        Ok(LogOutput::StdOut {
            message: Bytes::from_static(b"\"x\"}"),
        }),
        Ok(LogOutput::StdErr {
            message: Bytes::from_static(b"Done"),
        }),
    ];
    let heard = Mutex::new(Vec::new());
    let output = read_output(Box::pin(stream::iter(chunks)), &|line: &str| {
        heard.lock().expect("lock").push(line.to_owned());
    })
    .await
    .expect("reads");

    assert_eq!(output.stdout, b"{\"model\":\"x\"}");
    assert_eq!(
        heard.into_inner().expect("lock"),
        ["Loaded the model", "Transcribed 0:05:00 of 0:18:14", "Done"]
    );
    assert_eq!(
        output.stderr_tail,
        "Loaded the model\nTranscribed 0:05:00 of 0:18:14\nDone"
    );
}

#[test]
fn a_failures_output_keeps_its_end() {
    assert_eq!(tail("abcdef", 3), "def");
    assert_eq!(tail("ab", 3), "ab");
    assert_eq!(tail("aé", 1), "", "never cuts inside a character");
}

/// Opt-in: real transcriptions through a Docker proxy, as the engine runs them.
/// `WHISPER_TEST_DOCKER_URL` names a `docker-socket-proxy` (or the daemon), `WHISPER_TEST_IMAGE`
/// an image built from `whisper/`, and `WHISPER_TEST_RECORDING` a short recording of speech.
/// Checks a transcript comes back with timed words, that a file with no audio and one that is
/// no recording at all are told apart, and that no container is left behind.
#[tokio::test]
#[ignore = "needs WHISPER_TEST_DOCKER_URL, WHISPER_TEST_IMAGE, and WHISPER_TEST_RECORDING"]
async fn a_recording_is_transcribed_in_a_container_with_no_network() {
    // `verify-migrations.sh` runs every ignored test; this one needs Docker and an image.
    let (Ok(url), Ok(image), Ok(recording)) = (
        std::env::var("WHISPER_TEST_DOCKER_URL"),
        std::env::var("WHISPER_TEST_IMAGE"),
        std::env::var("WHISPER_TEST_RECORDING"),
    ) else {
        eprintln!(
            "skipped: set WHISPER_TEST_DOCKER_URL, WHISPER_TEST_IMAGE, and \
             WHISPER_TEST_RECORDING to transcribe through Docker"
        );
        return;
    };
    let docker =
        Docker::connect_with_http(&url, 3600, bollard::API_DEFAULT_VERSION).expect("a client");
    let mut whisper = Whisper::new(docker.clone(), image);
    // The development machine's cap; the engine derives its own.
    whisper.threads = 4;

    let bytes = std::fs::read(&recording).expect("the recording reads");
    let chunks = bytes
        .chunks(64 * 1024)
        .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
        .collect::<Vec<_>>();
    let key = Uuid::now_v7();
    let heard = Mutex::new(Vec::new());
    // Names to listen for reach the transcriber as its own argument.
    let hint_words = ["Elysium".to_owned(), "Sharath".to_owned()];
    let transcribed = whisper
        .transcribe(
            key,
            &hint_words,
            async { Ok(stream::iter(chunks)) },
            |line| {
                heard.lock().expect("lock").push(line.to_owned());
            },
        )
        .await
        .expect("transcribes");
    let Transcribed::Transcript(transcript) = transcribed else {
        panic!("speech has audio");
    };
    assert!(transcript.duration_ms > 0);
    let words: Vec<&WhisperWord> = transcript
        .segments
        .iter()
        .flat_map(|segment| &segment.words)
        .collect();
    assert!(!words.is_empty(), "it heard words");
    assert!(
        words
            .windows(2)
            .all(|pair| pair[0].start_ms <= pair[1].start_ms),
        "words are in order"
    );
    assert!(
        heard
            .lock()
            .expect("lock")
            .iter()
            .any(|line| line.starts_with("Loaded Whisper large-v3")),
        "progress arrived as it ran"
    );
    let left = docker.inspect_container(&container::name(key), None).await;
    assert!(
        matches!(left, Err(ref error) if has_status(error, 404)),
        "the container is removed: {left:?}"
    );

    // A picture is a file FFmpeg reads that has no sound.
    let mut picture = Vec::new();
    image::codecs::png::PngEncoder::new(&mut picture)
        .write_image(&[0, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .expect("a picture encodes");
    let silent = whisper
        .transcribe(
            Uuid::now_v7(),
            &[],
            async {
                Ok(stream::iter([Ok::<_, std::io::Error>(Bytes::from(
                    picture,
                ))]))
            },
            |_line| {},
        )
        .await
        .expect("a file with no audio is not a failure");
    assert_eq!(silent, Transcribed::NoAudio);

    let garbage = whisper
        .transcribe(
            Uuid::now_v7(),
            &[],
            async {
                Ok(stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(
                    b"no recording at all",
                ))]))
            },
            |_line| {},
        )
        .await;
    assert!(
        matches!(garbage, Err(WhisperError::Unreadable(_))),
        "{garbage:?}"
    );
}

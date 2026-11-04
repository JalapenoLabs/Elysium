// Copyright © 2026 Jalapeno Labs

//! The container one transcription runs in, and the limits it runs under.
//!
//! | Limit        | Value                              | Why                                                   |
//! |--------------|------------------------------------|-------------------------------------------------------|
//! | Network      | none                               | Nothing it holds may leave the host                   |
//! | CPUs         | its threads                        | It decodes on exactly that many                       |
//! | CPU weight   | [`CPU_SHARES`], a quarter of the default | Everything else on the host wins under contention |
//! | Memory       | [`MEMORY_BYTES`], with no swap     | The model and a long meeting's audio, with room       |
//! | Processes    | [`PIDS_LIMIT`]                     | Its threads and nothing like a fork bomb              |
//! | Privileges   | every capability dropped, no new ones | It reads a file and does arithmetic                |
//!
//! `whisper/measure.sh` runs the image under the same limits; change them together.

use std::collections::HashMap;

use bollard::models::{ContainerCreateBody, HostConfig};
use uuid::Uuid;

/// What a transcription's container is labelled with, so it reads as Elysium's in `docker ps`.
const MANAGED_LABEL: (&str, &str) = ("dev.elysium.managed", "whisper");

/// A quarter of Docker's default weight of 1024: when the API, Postgres, or an agent's turn
/// wants the same cores, they get four times the share a transcription does.
const CPU_SHARES: i64 = 256;

/// Whisper large-v3 and its working memory peaked at 5.0 GiB on 4 threads (`docs/meetings.md`,
/// Transcription), whatever the recording's length, and the audio it decodes to adds about
/// 230 MB per hour: a full day's recording fits. Beyond this the container is stopped, rather
/// than pushing the host into swap.
pub(super) const MEMORY_BYTES: i64 = 12 << 30;

/// `CTranslate2`'s and ONNX Runtime's threads, with room to spare.
const PIDS_LIMIT: i64 = 256;

/// The container a transcription keyed `key` runs in.
pub(super) fn name(key: Uuid) -> String {
    format!("elysium-whisper-{key}")
}

/// A container of `image` that decodes on `threads` threads, with nothing to reach and nothing
/// shared, reading the recording on its standard input.
pub(super) fn spec(image: &str, threads: usize) -> ContainerCreateBody {
    // A thread count is a small number; it fits any integer type.
    let threads_i64 = i64::try_from(threads).expect("a thread count fits in i64");
    ContainerCreateBody {
        image: Some(image.to_owned()),
        cmd: Some(vec!["--threads".to_owned(), threads.to_string()]),
        labels: Some(HashMap::from([(
            MANAGED_LABEL.0.to_owned(),
            MANAGED_LABEL.1.to_owned(),
        )])),
        // Standard input stays open for the one attach that streams the recording, and closes
        // when that attach closes its end, which is how the transcriber sees the end of it.
        open_stdin: Some(true),
        stdin_once: Some(true),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        host_config: Some(HostConfig {
            network_mode: Some("none".to_owned()),
            nano_cpus: Some(threads_i64 * 1_000_000_000),
            cpu_shares: Some(CPU_SHARES),
            memory: Some(MEMORY_BYTES),
            // The same as `memory`: no swap on top of it.
            memory_swap: Some(MEMORY_BYTES),
            pids_limit: Some(PIDS_LIMIT),
            cap_drop: Some(vec!["ALL".to_owned()]),
            security_opt: Some(vec!["no-new-privileges".to_owned()]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

# Copyright © 2026 Jalapeno Labs

# Whisper: transcribes a meeting's recording, word by word, on the host's own CPUs. The engine
# runs one container of it per transcription with no network at all (api/src/whisper/), the
# recording on standard input and the transcript on standard output (transcribe.py). See
# docs/meetings.md, Transcription.
#
# Everything is pinned and verified: the base image by digest, the model by its Hugging Face
# revision and each file's SHA-256, and every Python package by hash. The model is about 3 GB,
# so it is the first layer after the base: it changes least, and a change to anything below it
# never downloads, rebuilds, or pushes it again.
FROM python:3.13.18-slim-trixie@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1

# Whisper large-v3, converted for CTranslate2 by faster-whisper's authors, at a fixed revision.
# `ADD --checksum` refuses any file whose SHA-256 differs, and makes the checksum the layer's
# cache key, so a rebuild never downloads it again. The directory is made first, because ADD
# gives the directories it creates the files' mode too, which would leave them unsearchable.
RUN mkdir -p /opt/whisper/model
ARG MODEL_REVISION=edaa852ec7e145841d8ffdb056a99866b5f0a478
ARG MODEL_URL=https://huggingface.co/Systran/faster-whisper-large-v3/resolve/${MODEL_REVISION}
ADD --chmod=0444 \
    --checksum=sha256:69f74147e3334731bc3a76048724833325d2ec74642fb52620eda87352e3d4f1 \
    ${MODEL_URL}/model.bin /opt/whisper/model/model.bin
ADD --chmod=0444 \
    --checksum=sha256:a9306624f5ec14270a014b647e5c316b6e03a662c369758d1b90697a7b0655b9 \
    ${MODEL_URL}/config.json /opt/whisper/model/config.json
ADD --chmod=0444 \
    --checksum=sha256:7ccc62c6f2765af1f3b46c00c9b5894426835a05021c8b9c01eecb6dfb542711 \
    ${MODEL_URL}/preprocessor_config.json /opt/whisper/model/preprocessor_config.json
ADD --chmod=0444 \
    --checksum=sha256:6d8cbd7cd0d8d5815e478dac67b85a26bbe77c1f5e0c6d76d1ce2abc0e5f21ca \
    ${MODEL_URL}/tokenizer.json /opt/whisper/model/tokenizer.json
ADD --chmod=0444 \
    --checksum=sha256:c69260f2ab26d659b7c398f9a2b2b48ed0df16c3b47d7326782fd9cba71690c1 \
    ${MODEL_URL}/vocabulary.json /opt/whisper/model/vocabulary.json

# faster-whisper and what it needs, as wheels alone: CTranslate2 runs the model, PyAV (with
# FFmpeg's libraries inside its wheel) decodes any recording, and ONNX Runtime runs the voice
# activity filter.
COPY requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir --require-hashes --only-binary :all: \
      --root-user-action ignore --disable-pip-version-check \
      --requirement /tmp/requirements.txt \
  && rm /tmp/requirements.txt

# Nothing here needs root. The recording is spooled to /tmp, in the container's own layer,
# which goes with the container.
RUN useradd --uid 1000 --no-create-home --shell /usr/sbin/nologin whisper

# The container has no network, so nothing could be sent anyway: HF_HUB_OFFLINE keeps the Hub
# client from trying, and ORT_DISABLE_TELEMETRY turns off ONNX Runtime's usage telemetry, which
# otherwise starts at import and warns that it cannot store a device id.
ENV HF_HUB_OFFLINE=1 \
    ORT_DISABLE_TELEMETRY=1 \
    WHISPER_MODEL_DIRECTORY=/opt/whisper/model \
    WHISPER_MODEL_NAME="Whisper large-v3 (Systran/faster-whisper-large-v3@${MODEL_REVISION})"

COPY transcribe.py /opt/whisper/transcribe.py

USER whisper
# Isolated (-I), so nothing in the environment or the working directory changes what Python
# imports; no bytecode (-B), since /opt is read-only to this user.
ENTRYPOINT ["python", "-I", "-B", "/opt/whisper/transcribe.py"]

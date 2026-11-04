# Copyright © 2026 Jalapeno Labs

# The embedding model: turns the knowledge base's passages, and searches' queries, into the
# vectors meaning is searched by, on the host's own CPUs. Compose runs it as the `embeddings`
# service, on a network with no route out that only the engine and the API join. See
# docs/knowledge.md, Embeddings.
#
# Hugging Face's text-embeddings-inference (TEI) serves nomic-embed-text-v1.5 through ONNX
# Runtime. Everything is pinned and verified: TEI's CPU image by digest, and each model file
# by its SHA-256 at a fixed Hugging Face revision. The weights are about 550 MB, so they are the
# first layer after the base: they change least, and a change to anything below never
# downloads, rebuilds, or pushes them again.
FROM ghcr.io/huggingface/text-embeddings-inference:cpu-1.9.4@sha256:2538ea1c9640d3763b15af668039d24172d063b42337b0c27796fc2be180c78d

# nomic-embed-text-v1.5 at revision e5cf08aa, on purpose rather than the newest. The newest
# (e9b67630, "v5 Transformers") has the same weights byte for byte, but its config.json names
# the hidden size twice (`n_embd` and `hidden_size`), which TEI 1.9.4's ONNX backend refuses to
# read. Only the ONNX weights are added, not model.safetensors: with them TEI would fall back
# to its Candle backend on a config it cannot read, which runs this model ten times slower,
# rather than refusing to start. `ADD --checksum` refuses any file whose SHA-256 differs, and
# makes the checksum the layer's cache key, so a rebuild never downloads it again. The
# directories are made first, because ADD gives the directories it creates the files' mode
# too, which would leave them unsearchable.
RUN mkdir -p /opt/embeddings/model/onnx /opt/embeddings/model/1_Pooling
ARG MODEL_REVISION=e5cf08aadaa33385f5990def41f7a23405aec398
ARG MODEL_URL=https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/${MODEL_REVISION}
ADD --chmod=0444 \
    --checksum=sha256:147d5aa88c2101237358e17796cf3a227cead1ec304ec34b465bb08e9d952965 \
    ${MODEL_URL}/onnx/model.onnx /opt/embeddings/model/onnx/model.onnx
ADD --chmod=0444 \
    --checksum=sha256:0168e0883705b0bf8f2b381e10f45a9f3e1ef4b13869b43c160e4c8a70ddf442 \
    ${MODEL_URL}/config.json /opt/embeddings/model/config.json
ADD --chmod=0444 \
    --checksum=sha256:d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66 \
    ${MODEL_URL}/tokenizer.json /opt/embeddings/model/tokenizer.json
ADD --chmod=0444 \
    --checksum=sha256:d7e0000bcc80134debd2222220427e6bf5fa20a669f40a0d0d1409cc18e0a9bc \
    ${MODEL_URL}/tokenizer_config.json /opt/embeddings/model/tokenizer_config.json
ADD --chmod=0444 \
    --checksum=sha256:5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a \
    ${MODEL_URL}/special_tokens_map.json /opt/embeddings/model/special_tokens_map.json
ADD --chmod=0444 \
    --checksum=sha256:f93c503b32fa7bc2e186843f0add961caa14eb68f7be516e327df3cadbcd541a \
    ${MODEL_URL}/modules.json /opt/embeddings/model/modules.json
ADD --chmod=0444 \
    --checksum=sha256:9f40185cc7d04eb95cfc50571d1abde0dce45fb59d9c1321ef4b599d152cef4d \
    ${MODEL_URL}/sentence_bert_config.json /opt/embeddings/model/sentence_bert_config.json
# Mean pooling over the tokens, as the model was trained.
ADD --chmod=0444 \
    --checksum=sha256:3d6180f7233df2f529d65ea60c8db4af614fb78cbe99fd81103dcfeb9f8166a5 \
    ${MODEL_URL}/1_Pooling/config.json /opt/embeddings/model/1_Pooling/config.json

# Nothing here needs root. TEI writes only its socket between the router and the model, under
# /tmp, which compose mounts as a tmpfs over a read-only root.
RUN useradd --uid 1000 --no-create-home --shell /usr/sbin/nologin embeddings

# The container's network has no route out, so nothing could be fetched anyway: HF_HUB_OFFLINE
# keeps the Hub client from trying. The served model name is what /info answers and what each
# embedding is recorded as having been made by (`knowledge_passages.embedding_model`); the
# engine checks it against the model it expects (api/src/embeddings/) before it writes one.
ENV HF_HUB_OFFLINE=1 \
    PORT=8080 \
    MODEL_NAME="nomic-ai/nomic-embed-text-v1.5@${MODEL_REVISION}"

USER embeddings

# --max-batch-tokens 2048 is the context the model was trained on, and with --auto-truncate
# also the longest input: a passage is about 400 tokens (knowledge::passages), so only a rare
# longer one is cut. It also bounds memory: at TEI's default of 16,384 the attention over an
# 8,192 token warm-up took ONNX Runtime to 7.8 GiB, where this stays near 1.5 GiB.
ENTRYPOINT ["/bin/sh", "-c", "exec text-embeddings-router --model-id /opt/embeddings/model \
  --served-model-name \"$MODEL_NAME\" --max-batch-tokens 2048 --auto-truncate \
  --uds-path /tmp/text-embeddings-inference-server --json-output"]

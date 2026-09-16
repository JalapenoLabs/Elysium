# Environment variables

Environment variables are global settings every coding session's agent receives, such as a package registry token or
a build flag. They are managed on the Environment variables settings page (`/settings/environment`) and passed into
each session's satellite thread when the thread is created.

## Who can read a value

Every variable is placed in the agent's environment. The agent can read any value, print it, and send it anywhere its
tools reach, whether or not the variable is secret. Anything that must stay hidden from the agent does not belong
here.

`isSecret` decides two things, and nothing more:

- Elysium never returns the value again. The API answers `value: null` for a secret, and the form cannot show it.
- The satellite scrubs the value from the thread's output: logs, events, and rendered commands.

A non-secret variable's value is returned as plaintext by the API and shown in the table.

## Variables

| Field         | Meaning                                                                               |
|---------------|---------------------------------------------------------------------------------------|
| `key`         | The variable's name: a letter or underscore, then letters, digits, or underscores; up to 128 characters, unique |
| `value`       | Up to 32 KiB, kept exactly as sent, whitespace included; sealed in `environment_variables.value_encrypted` |
| `isSecret`    | Whether the value is write-only and scrubbed from thread output                       |
| `description` | Up to 500 characters, defaults to empty                                               |

Every value is sealed, secret or not, with the context `environment_variables.value:<id>`. One path for every row
keeps the table uniform and lets a variable become secret without its value moving. See `docs/secrets.md`.

## Refused keys

Some keys cannot mean anything useful in a thread, so Elysium refuses to store them and answers `400` with a message
naming the key and the rule. The rules live in one lookup table, `REFUSED_KEYS` in `api/src/environment/mod.rs`, read
through `key_refusal(key) -> Option<KeyRefusal>`. The frontend mirrors it in
`src/pages/Settings/Environment/environmentPresentation.ts` with translated messages. Keys match with their exact case.

| Rule                    | Keys                                                                                    | Why |
|-------------------------|-----------------------------------------------------------------------------------------|-----|
| `Malformed`             | Anything outside the key shape above                                                    | A process cannot carry the name |
| `ReservedForElysium`    | `GH_TOKEN`, `GITHUB_TOKEN`, and every key starting with `GIT_CONFIG_` (including `GIT_CONFIG_PARAMETERS`) | Elysium sets these for GitHub |
| `ReservedForSatellite`  | Every key starting with `ARSOX_`                                                         | The satellite refuses them: they hold its own secret |
| `ProviderCredential`    | A key starting with `ANTHROPIC_`, `OPENAI_`, `AWS_`, `AZURE_`, `GOOGLE_`, or `DEEPSEEK_` that contains `API_KEY`, `AUTH_TOKEN`, `ACCESS_KEY`, `SECRET`, or `CREDENTIALS` | The satellite refuses them, since its LLM proxy presents provider credentials on the agent's behalf |
| `OverriddenBySatellite` | `PATH`, `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` and their lowercase forms, `ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL` | The satellite sets these after every declared variable, so a stored value would never reach the agent |

The two satellite rules mirror arsox-satellite's `declared_key_refusal` and `is_provider_credential` in
`crates/arsox-satellite/src/harness/spawn.rs`, and the overridden keys its `environment_for`. Keep them in step when
the satellite changes. A shape rule rather than a list catches provider keys neither side has named.

## Blank values

A non-secret variable may have an empty value: it is set, to nothing, which is how a process reads a flag that only
needs to exist. A secret may not, and the API answers `400` whenever a secret would end up empty. The satellite scrubs
a secret's value from thread output, and the empty string gives it nothing to match; a blank secret field in the form
also means "keep the stored value", so an empty secret is never what someone meant.

## Changing a variable

`PATCH` accepts any subset of `key`, `value`, `isSecret`, and `description`. An absent `value` keeps the sealed one.

| Request                                       | Stored     | Result |
|-----------------------------------------------|------------|--------|
| No `value`, `isSecret` absent or unchanged    | Either     | The value is kept |
| No `value`, `isSecret: true`                  | Not secret | The flag changes and the value is kept; `400` if that value is empty |
| No `value`, `isSecret: false`                 | Secret     | `400`: a secret is never revealed by changing the flag |
| `value` given                                 | Either     | The value is re-sealed under the same row |
| `value: ""` where the result is secret        | Either     | `400` |
| Nothing that would change a column            | Either     | `400` |

Making a secret visible takes its value again, which proves the client already knows it. The form mirrors this: while
editing a secret, the value field is optional and keeps the stored value, and it turns required the moment the Secret
switch is turned off.

## Reaching a thread

`thread_environment(connection, cipher) -> anyhow::Result<Vec<ThreadVariable>>` in
`api/src/models/environment_variable.rs` returns every variable decrypted, ordered by key, as
`ThreadVariable { key, value: SecretString, is_secret }`. It is the only way code reads the whole environment. A
value that will not open fails the call naming the key, never the value, and the session is not created.

Creating a coding session puts every variable into `ThreadSettings.env` with its `is_secret` flag, followed by the
variables Elysium sets itself for GitHub (`GH_TOKEN` and `GIT_CONFIG_*`, see `docs/github.md`). Elysium's names are
refused as workspace variables, so neither list overrides the other.

Changes apply to threads created afterwards. A running thread keeps the environment it was created with, and deleting
a variable does not remove it from one.

## Roadmap

- Variables scoped to projects, linked the way storage locations are.

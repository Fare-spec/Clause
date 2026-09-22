# Clause

Rust Discord bot using Serenity. It supports interactive server setup, scoped
manager roles/channels, per-guild file storage, retained logs, curated rule JSON,
AI-assisted rule summaries, AI rule generation from manager-uploaded files, and
report-only AI review of messages in configured bot channels.

## Discord setup

1. Create an application and bot in the [Discord Developer Portal](https://discord.com/developers/applications).
2. Under **Bot → Privileged Gateway Intents**, enable **Message Content Intent**.
3. Invite the bot to your server using the `bot` and `applications.commands`
   scopes, with **View Channels** and **Send Messages** permissions.
4. Set the `DISCORD_TOKEN` environment variable to the bot token. Keep it out of
   source control.

## Run locally

With Rust installed and `DISCORD_TOKEN` set:

```sh
cargo run --locked
```

Send `!ping` in a server channel the bot can access.

## Run with Docker

With `DISCORD_TOKEN` exported in your shell:

```sh
docker build -t clause .
docker run --rm --name clause --env DISCORD_TOKEN clause
```

Or copy `.env.example` to `.env`, fill in `DISCORD_TOKEN`, optionally set
`COMMAND_GUILD_ID` for one test server, and run Compose:

```sh
docker compose up --build
```

The Compose setup stores SQLite in the `clause-data` volume and guild uploads,
retained logs, and quota metadata in the `clause-guilds` volume. Use
`docker compose down` to stop the bot while keeping data, or
`docker compose down -v` to remove the volumes.

The container runs as a non-root user and requires no inbound ports. Stop it with
`docker stop clause` or `docker compose stop`; the bot handles SIGTERM and Ctrl+C
for graceful shutdown.

Slash commands are registered globally by default when the bot becomes ready.
For development or a single test server, set `COMMAND_GUILD_ID=<guild-id>` before
startup to register the current command set directly in that guild. Guild command
registration is much easier to see while iterating; global commands are the
release default.

AI features require an OpenAI-compatible chat endpoint in `.env`:

```sh
API_KEY=...
AI_ENDPOINT_URL=https://api.openai.com/v1/chat/completions
AI_MODEL=gpt-4o-mini
```

Without those values, the bot still starts. `/summary`, `/rules generate`,
`/rules from-channel`, and message review report or silently skip AI work when AI
is not configured. Bot managers can override the endpoint, model, and API key for
one server with `/ai set`; `/ai show` never reveals the stored key.


## GitHub Container Registry image

The GitHub Actions workflow in `.github/workflows/docker-image.yml` builds the
Dockerfile on pull requests and publishes an image to GitHub Container Registry
on pushes to `main`, `master`, and `v*.*.*` tags. Published images are tagged
with the branch name, Git tag, commit SHA, and `latest` on the default branch.

After pushing to GitHub, pull the image with:

```sh
docker pull ghcr.io/fare-spec/clause:latest
```

To run the published image with Compose instead of building locally, use the
provided GHCR Compose file:

```sh
docker compose -f docker-compose.ghcr.yml pull
docker compose -f docker-compose.ghcr.yml up -d
```

That file uses `ghcr.io/fare-spec/clause:latest` and keeps the same `.env`,
`clause-data`, and `clause-guilds` volumes as the local build compose file.

Keep `DISCORD_TOKEN`, `API_KEY`, and provider settings in `.env` or deployment
secrets. They are not needed at image build time and should not be baked into the
image.

## Configure a server

Run `/setup` as a server administrator or a member with **Manage Server**.
The private Discord setup uses two pages with dropdowns, Back/Next, and Save/Cancel:

- **Manager roles:** select 1–25 roles. Members of any selected role can use `!ping` and `!config`.
  Administrators and members with Manage Server always retain access.
- **Bot channels:** select 1–25 text channels where commands work after setup.
  When AI is configured and curated rules exist, ordinary messages in these
  channels are reviewed against the rules.
- **Log channel:** the destination for this guild’s logs. The bot needs View Channel,
  Send Messages, and Embed Links here.
- **Logging level:** off, error, warn, info (default), or debug.
- **Disk retention (page 2):** none (default), flagged/managed messages for
  7/30/90 days, or all messages and bot actions for 1/7/30 days.

Select your settings and click **Save settings**. Clause checks that it can
view and send messages in every selected channel. The single log channel may
also be one of the bot channels.
Settings are saved together in SQLite (`DATABASE_PATH`, default `bot.db`) and
survive restarts. Cancel leaves previously saved settings intact.

Run `/setup` again to edit existing settings. Only one administrator can configure
a server at a time; reopening your own setup replaces your previous panel. Panels
expire after 15 minutes or a bot restart. Only administrators and members with
Manage Server can change setup, including the manager roles.
Existing single-role/channel configurations migrate automatically on startup.


## Guild storage

Saving `/setup` creates `guilds/<guild-id>/`, using the stable guild ID as the
folder name. Uploaded files are stored in `uploads/` and retained logs in `logs/`.
Set `GUILD_STORAGE_PATH` to change the root directory. Existing
files survive repeat setup and cancellation does not create a folder.

Each folder contains `storage-limit.json` recording a **50 MB**
limit per guild, including uploads, retained logs, and metadata. Registration and uploads check recursive file usage and refuse writes above
the limit. Quota checks and file writes are serialized within the bot process.
This is an application limit, not an OS filesystem quota; run only one bot
process against a storage directory. Setup does not
delete files to make room. If saving the database fails after folder creation,
the folder is retained for a retry.

For Docker, persist `/app/guilds` with a volume (for example,
`--mount type=volume,src=clause-guilds,dst=/app/guilds`). A custom storage path
must be writable by the container user (UID 10001).


**README generated by AI**


## Manage guild files

Members with any configured manager role, administrators, and members with
Manage Server can use these commands in a configured bot channel:

- `/files list [page]`: browse upload filenames, sizes, and total guild usage (10 files per page).
- `/files view filename`: download a file to inspect its contents.
- `/files upload file`: select a Discord attachment to save in the guild folder.
- `/files remove filename`: permanently delete the named file.

All responses are private. `storage-limit.json` is listed and readable but cannot
be uploaded, replaced, or removed through these commands. The 50 Mo limit is
fixed by the bot, not by user-supplied JSON. Uploads include metadata in total
usage and never overwrite existing files; remove a file first to replace it.
Removing files is allowed even when the folder is over quota.

Filenames may contain ASCII letters, numbers, spaces, dots, hyphens, and
underscores, up to 100 characters. Leading dots, trailing dots/spaces, paths,
subfolders, and symlinks are not accepted by file commands. Downloads are also
subject to Discord's attachment size limit. Files are stored as data and are
never executed by the bot.

## Tests

Run `cargo test --locked` (no Discord token or network access required).
Dedicated tests in `src/storage/tests.rs` and `src/files/tests.rs` cover file
operations, guild isolation, protected metadata, quota boundaries, unsafe paths,
symlinks on Unix, and manager/channel authorization. Setup persistence and
migration tests remain in `src/setup.rs`. Filesystem tests use isolated temporary
directories and sparse files for quota checks; they do not touch `bot.db` or
production guild folders.


## Guild logging

Choose a level in `/setup`; it is saved independently for each guild:

| Level | Records |
| --- | --- |
| `off` | No guild logs |
| `error` | Failed bot operations and response delivery |
| `warn` | Errors plus rejected file actions, permission denials, and invalid requests |
| `info` | Warnings/errors plus setup responses, file actions, command replies, and connection notices |
| `debug` | All of the above plus incoming guild gateway events, including message content |

Logs go only to that guild’s configured log channel. The bot verifies channel
ownership before delivery and disables mentions in logs. Restrict the log
channel to trusted members using Discord permissions: debug can copy content
from other channels the bot can access. The bot does not change channel
permissions automatically. Guild payloads are not written to console logs. Disk retention is configured
separately; retained logs count toward the same 50 Mo quota as uploads.

Debug covers events available through the bot’s enabled gateway intents; it
cannot see inaccessible channels or events Discord does not send. DMs, global
multi-guild events, credentials, forwarded/referenced message bodies, the bot’s
own messages, and incoming events in the log channel are excluded. Large entries
are truncated to fit Discord embeds. Delivery uses a bounded queue; records may
be dropped if it fills or Discord delivery fails. Log sending never logs itself.
Queued entries are checked against current settings again before delivery.

Logging tests are in `src/logging/tests.rs`, including guild routing, filtering,
credential redaction, response auditing, and Unicode length limits. Database
migration tests also verify the default level and persistence after restart.


## Disk retention and upload folders

```text
guilds/<guild-id>/
  storage-limit.json   # protected quota metadata
  uploads/             # /files upload, list, view, remove
  logs/                # bot-owned retained JSON records
```

Page 2 of `/setup` asks what to retain and for how long. The default **None**
keeps no records on disk. **Flagged / managed** retains only messages that Clause
flags or marks as gray-area during AI rule review, plus the review result.
**All** retains observed message create/update/delete events and bot action records. It does not fetch historical messages or download
message attachments. Incoming data has the same guild isolation, redaction, and
log-loop exclusions as debug logging. Deletion events contain only what Discord
provides, which may just be message IDs.

Retention is independent of Discord verbosity: `off` can still retain messages
if an All policy is selected. Retained records preserve their text without the
Discord embed truncation. Queues are bounded and delivery/storage failures can
drop records; this is not a guaranteed audit archive.

Cleanup runs on startup, every minute (including idle guilds), before uploads,
and when settings are saved. Records expire based on their capture time and the
current policy. Choosing **None** removes existing bot-owned retained records;
switching to flagged-only removes ordinary message/action records; shortening
the duration expires older records. Uploads, quota metadata, unknown files, and
messages already posted to Discord are never deleted by retention cleanup.

Uploads and log writes share one quota check and lock. Expired logs are removed
before checking space; when the folder is full, new retained records are skipped
and uploads are rejected. Unexpired logs and uploaded files are not evicted.
`/files list` shows the combined usage, but file commands cannot alter retained
logs or the size-limit JSON. Retained records are available in the host's guild
folder, not through `/files`.

Existing root-level upload files migrate automatically into `uploads/` on startup
or first access. Migration preserves their contents and quota usage. If both
locations already contain the same filename, migration stops without overwriting
either file; resolve the collision on the host. Reserved folder names must be
real directories, not files or symlinks.

## AI rule review

When AI is configured and a guild has curated rules, Clause reviews ordinary
messages in configured bot channels against `rules/rules.json`. A compliant
message produces no public response. A clear violation creates a warning log in
the configured log channel, pings the configured manager roles, records it as a
managed message if retention allows it, and replies in the original channel with
an advisory explanation and matching rule IDs.

If the AI says the message is a gray area, Clause uses the same report and ping
flow but the channel reply says staff were asked to review it because the bot is
unsure. Clause does not delete messages, timeout users, ban users, or apply any
automatic punishment. Managers decide what to do.

The AI request contains the current message content, attachment metadata
(filename, type, size), and the curated rules JSON. It does not download ordinary
message attachments for review. If a manager configured a server-specific AI
provider with `/ai set`, that provider is used for summaries, rule generation,
channel imports, and message review in that server.

## Storage status and privacy information

- `/storage`: private response with available/used bytes and an uploads, retained
  logs, and metadata/other breakdown after retention cleanup. Any server member
  can request the aggregate; no file names or log contents are exposed.
- `/settings`: private response showing this server's log level, retention policy,
  log channel, bot channels, manager roles, storage summary, and privacy contact.
  Any server member can use it to understand what Clause is configured to do.
- `/summary`: private response with an AI-generated Markdown summary of the
  current curated rules JSON. Any server member can request it when the rules are
  public; bot managers can always request it. The bot sends only
  `rules/rules.json`, not arbitrary uploads, to the configured AI endpoint.
- `/rules list`: private response with a Markdown export of the curated rules
  when rules are public or the requester can manage the bot.
- `/rules generate`: manager-only regeneration of the curated rules JSON from
  files currently stored in `uploads/`. Clause sends those upload contents to the
  configured AI endpoint, validates the returned JSON rules, preserves the current
  public/private visibility setting, and replaces `rules/rules.json` only after a
  valid response.
- `/rules from-channel channel [limit]`: manager-only regeneration of the curated
  rules JSON from up to 1,000 recent non-bot text messages in the selected channel.
  The AI is instructed to ignore messages that are not rules or rule
  clarifications.
- `/rules add id severity text`: manager-only add or replace of a rule.
- `/rules update id [severity] [text]`: manager-only update of an existing rule.
- `/rules remove id`: manager-only removal of a rule.
- `/rules visibility public`: manager-only toggle for whether ordinary members
  can inspect the rules.
- `/rules export`: manager-only JSON export of the curated rules document.
- `/ai show`: manager-only display of whether this server has an AI override, the
  endpoint, and the model. The key is never shown.
- `/ai set endpoint model api_key`: manager-only server-specific AI provider
  override. The endpoint must be an HTTPS OpenAI-compatible chat-completions URL.
- `/ai clear`: manager-only removal of the server AI override, falling back to
  environment AI settings.
- `/logs clear confirm:false`: private manager-only preview of retained local log
  usage.
- `/logs clear confirm:true`: private manager-only deletion of Clause-owned local
  retained logs. This frees guild quota but does not delete messages already
  posted in the Discord log channel. If retention still includes bot actions, the
  cleanup itself may leave a new small audit entry.
- `/privacy`: privacy policy attachment, current guild logging/retention settings,
  and the operator's private contact for access, correction, deletion, and reports.
- `/terms`: terms attachment and operator contact.

The default operator label is `Clause operator` and the private contact is
`git-spectre@proton.me`. Set `BOT_OPERATOR` and `PRIVACY_CONTACT` to override them;
independent operators must supply their own name and monitored private email
address or support URL. These values appear in policy commands. Empty values
produce an explicit incomplete-contact notice. Policies are embedded at build time; rebuild after editing them.
Publish stable public URLs for [PRIVACY.md](PRIVACY.md) and [TERMS.md](TERMS.md)
and set the corresponding fields in the Discord Developer Portal.

Raw logs remain restricted: public log access can expose other people's data.
Personal-data requests are handled manually by the operator; file-manager access
is not a substitute for a request channel available to ordinary users. Document
and actually operate identity verification, access/correction/deletion handling
(including Discord copies and backups), offboarding and shutdown cleanup. The
application does not yet automate those workflows or encrypt host files; use
encrypted storage at rest and restrict filesystem/backup access before public use.
Logging and All retention should only be enabled when needed for the disclosed
function, with appropriate member notice and authority.

Policy references checked on 22 September 2026:
[Discord Developer Terms, section 5](https://support-dev.discord.com/hc/en-us/articles/8562894815383-Discord-Developer-Terms-of-Service)
and [Discord Developer Policy](https://support-dev.discord.com/hc/en-us/articles/8563934450327-Discord-Developer-Policy).
The documents describe current functionality; they do not certify compliance of
a particular deployment with Discord's rules or applicable privacy law.


**README GENERATED BY AI**

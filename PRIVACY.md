# Clause Privacy Policy

Last updated: 23 September 2026

## Operator and contact

This policy describes the current Clause software. The Clause operator's private
contact is [git-spectre@proton.me](mailto:git-spectre@proton.me) for privacy requests,
data access, correction, deletion, support, and reports of misuse.

The operator of the particular bot installation is responsible for its operation
and handling of personal data. Use `/privacy` to see that installation's operator
label, private contact method, and current server logging and retention settings.
Separately hosted installations must publish their own operator/contact details;
the project maintainer cannot access data stored by independent operators.

If `/privacy` says the contact is not configured, ask the server owner to identify
its operator. That deployment is not ready for public use until its operator
publishes a working private contact method. Do not submit personal data in a
public issue tracker.

## Data processed and purposes

Clause uses Discord data to configure the bot, check permissions, answer commands,
manage server files, and provide server-authorized diagnostic logging. This can
include server, channel, role, user and message IDs; usernames and member roles;
command and interaction details; message text, timestamps, embeds, and attachment
metadata or URLs; and other guild event fields provided by Discord's enabled
intents. Debug logging can include events outside configured bot-command channels
where the bot has access. Clause does not fetch historical messages in bulk.

A manager's `/files upload` downloads the selected attachment and stores its bytes
and filename in that server's upload folder. Ordinary message attachments are not
automatically downloaded for retention. The bot does not execute uploaded files.
`/storage` shows aggregate usage, not filenames or message content. `/metrics show` is manager-only and shows aggregate local counts and sizes, such as upload count, retained log size, rule count, AI provider source, and whether metrics forwarding is allowed. It does not show message text, filenames, rule text, usernames, or API keys.

When AI is configured, `/ai test` sends one small diagnostic prompt to the active provider. `/summary` sends the curated rules JSON from this server's
`rules/rules.json` file to the configured AI endpoint to generate a rule summary.
`/rules generate` is manager-only and sends the current files in that server's
`uploads/` folder to the configured AI endpoint so it can extract rules and
replace `rules/rules.json`. `/rules from-channel` is manager-only and sends up to
100 recent non-bot text messages from a selected channel to the configured AI
endpoint so it can extract rule-relevant messages and replace `rules/rules.json`.
If a rule source channel is configured, each message posted there by a bot
manager is sent to the configured AI endpoint so Clause can decide whether it is
rule-relevant and add or update extracted rules. Messages from non-managers in
that channel are ignored by Clause. Clause also sends ordinary messages in
configured bot channels, plus attachment
metadata and the curated rules JSON, to the configured AI endpoint for
report-only rule review. Compliant messages produce no public
response. Clear violations and gray-area results are posted to the configured log
channel, ping bot-manager roles, and receive an advisory in-channel bot reply.
Clause does not automatically delete messages or punish users. The endpoint, model, account, and retention practices depend on the operator's AI
provider configuration. A bot manager can store a server-specific AI endpoint,
model, and API key with `/ai set`; the key is stored in SQLite, hidden from bot
responses, and redacted from Clause debug logs when known as `api_key`, `key`, or
`authorization`. Do not put secrets or unrelated personal data in uploads, rules,
rule-source channels, or bot-channel messages.

## Storage and retention

Server configuration is stored in SQLite, including role/channel IDs, the optional
rule source channel ID, log level, retention policy, the metrics forwarding preference, and any server-specific AI
provider override. Each server has a separate directory containing protected
quota metadata, an `uploads/` folder, and a `logs/` folder. Uploads, local retained
logs, and metadata share a configurable per-guild limit, defaulting to 10 Mo (10,000,000 bytes). SQLite configuration and
Discord-hosted messages are outside this file quota.

Disk retention defaults to **None**. Administrators can select:

- **None:** no local retained records; existing bot-owned retained logs are removed.
- **Flagged/managed:** messages that Clause flags or marks as gray-area during
  AI rule review, plus the review result, for 7, 30, or 90 days.
- **All:** observed message create/update/delete events and bot actions, for 1, 7,
  or 30 days. A deletion event may contain only IDs, not the deleted message text.

Local records expire from capture time under the current policy. Cleanup runs on
startup, approximately every minute while the bot is running, before file/storage
operations, and when setup is saved. Offline periods or storage errors can delay
physical deletion until cleanup succeeds. Shorter policies apply to existing
records. Configured bot managers can also run `/logs clear confirm:true` to delete
Clause-owned local retained logs early and free guild storage. Server administrators can run `/disable` to mark setup incomplete and clear retained local logs for that guild. The Discord server owner can run `/leave confirm:true delete_data:true` to delete that guild's local Clause settings and guild storage folder before the bot leaves. Uploads have no
automatic expiry; authorized managers can remove them, and the operator handles
applicable personal-data requests. Configuration and folders are not automatically
erased when the bot leaves a server; the operator must remove data when it is no
longer needed or when deletion is required.

The Discord log level is separate from local retention. `info` is the default;
`debug` includes incoming message content and events. Discord-channel log copies
are not removed by local retention and can remain after the source message is
edited or deleted. Turning logging off does not erase earlier Discord messages.
A data request must account for those copies as well as local files and any
operator-maintained backups.

## Access and recipients

Discord processes commands, responses, attachments and logs delivered through its
service under its own policies. The bot operator and its hosting providers can
access stored data as necessary to operate the deployment. Operators must disclose
any deployment-specific providers and restrict their access appropriately.

Discord channel permissions determine who can read posted logs. Server
administrators should restrict the log channel to authorized staff; debug output
can contain content from other accessible channels. Configured bot managers,
administrators and members with Manage Server can access uploads through file
commands. Local retained logs are not publicly downloadable through the bot and
are not exposed by `/files`. Raw logs may contain other members' personal data.

Clause routes guild logs only to that guild's configured channel and storage.
DMs and global multi-guild events are excluded from guild logs. Credentials in
known event fields and forwarded/referenced message bodies are redacted. These
filters cannot identify every secret someone includes in ordinary message text;
do not send passwords, tokens, or sensitive personal information to the bot.
The current application has no advertising, data-sale, or model-training feature. The current build stores a per-server metrics-forwarding preference but does not send metrics to a main server or external telemetry endpoint.

## Your data requests

You can use the private contact in `/privacy` to request information about your
own data, access to it, correction, or deletion, and to raise a privacy concern.
You do not need a bot-manager role to ask. Give your Discord user ID and relevant
server/message IDs where possible; do not send passwords, access tokens, or
unrelated private messages. The operator may verify identity using proportionate
means before releasing or changing data.

Requests are handled manually; there is no automatic personal-data export or
delete command. An access response should concern your data and protect other
people's data rather than publish a complete raw server log. The operator must
promptly address applicable requests, including relevant retained logs, uploaded
files, Discord log copies and backups, subject to applicable law. Automatic
expiry is not a substitute for responding to a deletion request. Contact the
operator if the bot has left the server or you can no longer use its commands.

## Security and policy changes

The current application writes ordinary JSON/files and SQLite; it does not itself
encrypt stored data. The operator must provide encrypted storage at rest,
restricted host access, and appropriate backup and incident-handling safeguards.
Do not assume that a private Discord channel encrypts the bot's host storage.

The operator must keep this policy and the deployment contact accurate, publish
an up-to-date public policy URL in Discord's Developer Portal, and make changes
available before materially changing data practices. Discord's own privacy policy
also applies to data processed by Discord.

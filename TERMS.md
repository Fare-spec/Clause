# Clause Terms of Service

Last updated: 23 September 2026

## Service and operator

Clause currently provides server setup, manager permissions, file storage,
logging, configurable local retention, curated rules JSON, AI generation of that
rules JSON from manager-uploaded files, manager-selected channel messages, or
manager messages in an optional rule source channel, AI summaries of the curated
rules JSON, configurable per-server AI providers, and
report-only AI review of messages in configured bot channels. The operator
identified by `/privacy` runs the particular installation and is responsible for
support.
The default Clause contact is
[git-spectre@proton.me](mailto:git-spectre@proton.me); independently hosted
installations must identify their own operator and contact. These terms are
between that operator and users of the installation, not Discord. Clause is not
endorsed by Discord.

Use must comply with applicable law, Discord's Terms of Service and Community
Guidelines. Operation of the bot must also comply with Discord's Developer Terms
and Developer Policy. These terms do not override those requirements or restrict
rights that cannot lawfully be waived. Users must meet Discord's applicable age
requirements.

## Authorized use

Only authorized server administrators or members with Manage Server may change
setup. Bot-manager roles grant the file access described by the commands; they
do not grant access to other servers' data. `/storage`, `/privacy`, and `/terms`
do not require a manager role. Discord integration permissions may still affect
command availability.

Upload only files you have the right to store and make available to authorized
server staff. Do not upload unlawful content, credentials, sensitive personal
information prohibited by Discord's developer rules, or material intended to
harm the service or its users. Do not evade permissions, quotas, retention
controls or protections on the size-limit metadata. Curated rules JSON may be
sent to the configured AI provider when `/summary` is used. Files in the upload
folder may be sent to the configured AI provider when a manager runs `/rules
generate` to regenerate the curated rules JSON. Recent messages in a
manager-selected channel may be sent to the configured AI provider when a manager
runs `/rules from-channel`. Messages posted by bot managers in a configured rule
source channel may be sent to the configured AI provider to decide whether they
contain rules and to merge extracted rules. Messages in configured bot channels,
attachment metadata, and curated rules JSON may be sent to the configured AI
provider for
rule review. AI review is advisory and report-only:
Clause may notify staff and reply with the matched rule explanation, but it does
not automatically delete messages or punish users. Do not use logs or AI outputs
for surveillance, harassment, profiling, scraping, advertising, data sales or
model training.

## Server administrator responsibilities

Administrators must explain enabled logging and retention to affected members,
provide access to the privacy policy and operator contact, and ensure they have
the necessary authority and legal basis for their configuration. Selecting a
setting is not consent on behalf of every person whose data may appear in it.
Use only the data and retention needed for the stated server function. Debug and
All retention and a configured rule source channel can capture message content
outside bot-command channels that the bot can access; limit the bot's channel
access and keep logs restricted to staff
with a legitimate need to see them. Do not redistribute raw logs to all members.

## Storage and retention limits

Each guild has a shared configurable allowance for uploads, retained logs and metadata. The default is 10 Mo unless the operator changes it before startup.
Uploads are stored separately from retained logs. The protected size-limit JSON
cannot be changed through file commands. Uploads do not overwrite existing files.
At capacity, uploads fail and new retained records may be skipped. The service is
not a backup service or a guaranteed audit archive.

Saving a None retention policy removes existing bot-owned local logs. Changing
the scope or shortening retention can also remove records. Expiry does not delete
uploaded files or log messages already posted to Discord. Configured bot managers
can use `/logs clear confirm:true` to delete Clause-owned local retained logs
early and free guild storage. Server administrators can use `/disable` to stop
Clause for the guild until setup is run again. `/files remove` permanently deletes the named
upload. The [Privacy Policy](PRIVACY.md) describes retention periods, cleanup
limitations, access and data requests.

## Privacy, requests, and support

The operator must maintain the contact displayed by `/privacy`, review reports of
misuse, and respond to applicable requests concerning users' data. A manager role
is not required to ask for access, correction, or deletion of your own data.
Requests are reviewed privately and manually; they do not entitle a requester to
other members' raw logs. Do not submit personal data in public support reports.

Before public deployment, the operator must publish accurate operator/contact
information and public privacy/terms URLs, disclose hosting arrangements, provide
encrypted storage at rest and appropriate access controls, and establish a
process for requests, data no longer needed, shutdown and security incidents.
Editing these documents does not, by itself, implement those operational duties.

## Availability and changes

Discord outages, rate limits, queue limits, storage errors and restarts may delay
or prevent commands, logging or cleanup. The operator may update, restrict or
stop the service, consistent with applicable law and privacy obligations. No
part of these terms excludes a responsibility or remedy that applicable law does
not allow to be excluded. Changes to service behavior or data practices must be
reflected in the published terms and privacy policy.

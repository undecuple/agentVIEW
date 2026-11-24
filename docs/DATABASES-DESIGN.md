# SQLite, DuckDB and coordinator database tools

Status: SQLite operational storage exists. DuckDB analytics and coordinator-facing
project database tools are requested designs, not connected capabilities.

## Internal data

Keep SQLite as the authoritative store for app records: hierarchy, conversations,
launch identities, jobs, access grants and audit metadata. Keep transactions small
and preserve the existing private storage, schema migration and writer-lease rules.
[SQLite's application guidance](https://sqlite.org/whentouse.html) supports this
local application role.

Add DuckDB as an optional analytical worker for resource/activity history and
usage reports when those measurements exist. Read consistent, revision-stamped
snapshots or exports; show their as-of time. Analytics must not write operational
approval/session state or present estimated usage as measured provider receipts.
Do not introduce two competing sources of truth or silently download extensions.
DuckDB's [concurrency documentation](https://duckdb.org/docs/stable/connect/concurrency.html)
describes its in-process concurrency and analytical workload model; keep the app's
initial analytical deployment local rather than adding a server by default.

Live SQLite files need a supported consistent snapshot/backup process, including
WAL state; copying only the main database file is not an acceptable backup scheme. (Today the app's own databases use SQLite's default journal mode, not WAL; this applies if that changes or to any database that uses WAL.)
The remote Web UI should call the PC service, not open either database file over
NetBird. No listener is enabled by this design.

## Project database tools

Provide a Data view where the user explicitly registers SQLite or DuckDB files
for a project and grants named coordinators access. Display engine, approved
source, project, schema, connection/read status and effective recipients. Use the
same explicit delegation model as the planned credential vault; parent access
does not automatically become subagent access.

Start with read-only schema browsing, bounded table previews and query results.
A coordinator should be able to request these through scoped app tools, using a
trusted connection ID rather than supplying arbitrary filesystem paths. Require
a separate reviewed flow for writes, migrations or imports, with a visible diff
or operation plan and appropriate backup/transaction handling.

Treat SQL as executable capability. Read-only does not by itself constrain file
reads, extensions or network access. Run analytical/query work with approved
filesystem access, external access/extension loading disabled by default, bounded
CPU/memory/time/results and cancellation. Credentials, if later needed, remain
vault references rather than values in prompts, query history or exports.

Acceptance uses disposable databases: cross-project/coordinator refusal, no
implicit child grants, read-only write refusal, external-file/network/extension
refusal, bounded previews, cancellation, correct snapshots and explicit stale
analytics status. Never test against a user's live project database by default.

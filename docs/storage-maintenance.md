# Local storage upkeep

The daemon runs maintenance at startup and hourly on its blocking pool. Each database pass
removes at most 1,000 rows per category:

- Notifications read more than 30 days ago, or any notification older than 90 days.
- Fired, non-recurring timers older than 7 days.
- Expired project leases.
- Detached actor sessions not seen for 90 days, unless they still own a lease or active timer.

Scratchpads, todos, and feedback records have no age-based retention. Closing an agent keeps
the existing deletion behavior; there is no agent archive. Removing a project from its last
profile deletes its canonical database records and notifications. Projects still registered
in another profile keep their data. Deleting a profile closes processes in its unshared projects
through the normal lifecycle first, releasing PTYs, attachment directories, and output buffers.

Maintenance also removes orphan process output, agent attachments, and feedback media/packets.
Feedback cleanup handles at most 100 directories per root per pass. It preserves registered
recordings, unknown paths, and symlink roots. Notification, timer, and feedback IDs remain
monotonic after cleanup so old delivery IDs and media paths cannot attach to new records.

New databases use incremental auto-vacuum and reclaim up to 256 free pages per pass. Existing
databases retain their current mode and reuse freed pages; upgrades do not run a blocking full
VACUUM. A passive WAL checkpoint and an 8 MiB journal size limit reduce retained journal space
when SQLite can reset it. The journal limit is not a hard cap while readers hold WAL snapshots.

SQLite documents [page reuse and incremental vacuum](https://www.sqlite.org/pragma.html#pragma_auto_vacuum)
and [WAL checkpoint behavior](https://www.sqlite.org/wal.html). Tests cover migration with existing
notification data, retention boundaries, live-state preservation, profile sharing, monotonic IDs,
and orphan-file cleanup using temporary databases and directories.

---
title: Cron Jobs
description: Schedule recurring EdgeCrab agent tasks with the built-in cron engine. Grounded in crates/edgecrab-cron/src/lib.rs and the manage_cron_jobs tool.
sidebar:
  order: 8
---

EdgeCrab includes a built-in cron scheduler. Scheduled jobs run the EdgeCrab agent on a recurring schedule — the same agent loop, same tools, same config.

---

## Cron Syntax

EdgeCrab uses standard 5-field cron syntax:

```
┌──── minute (0-59)
│ ┌──── hour (0-23)
│ │ ┌──── day of month (1-31)
│ │ │ ┌──── month (1-12)
│ │ │ │ ┌──── day of week (0-7, 0/7=Sunday)
│ │ │ │ │
* * * * *
```

Examples:

```
0 9 * * 1-5          # 9 AM weekdays
0 */4 * * *          # every 4 hours
0 0 1 * *            # midnight on 1st of each month
*/15 * * * *         # every 15 minutes
```

---

## Managing Cron Jobs

### CLI

```bash
# List all jobs
edgecrab cron list

# Add a job
edgecrab cron add "0 9 * * 1-5" "summarize overnight GitHub activity"

# Add with a name
edgecrab cron add --name morning-standup "0 9 * * 1-5" "morning standup summary"

# Enable/disable
edgecrab cron enable morning-standup
edgecrab cron disable morning-standup

# Delete
edgecrab cron delete morning-standup

# Run now (one-shot)
edgecrab cron run morning-standup
```

### Agent Tool (`manage_cron_jobs`)

The agent can manage its own schedule during a session:

```
❯ Schedule a daily report at 6 PM that summarizes today's git commits
```

The agent calls `manage_cron_jobs` with:

```json
{
  "action": "create",
  "name": "daily-git-report",
  "schedule": "0 18 * * *",
  "task": "Summarize today's git commits across all repositories in ~/projects/"
}
```

---

## Cron Storage

Jobs are stored in `~/.edgecrab/cron/`:

```
~/.edgecrab/cron/
├── morning-standup.json
├── daily-git-report.json
└── weekly-review.json
```

Each file is a JSON descriptor:

```json
{
  "name": "morning-standup",
  "schedule": "0 9 * * 1-5",
  "task": "Summarize overnight GitHub notifications and Slack messages",
  "enabled": true,
  "model": null,
  "toolsets": null,
  "last_run": "2025-05-14T09:00:00Z",
  "next_run": "2025-05-15T09:00:00Z"
}
```

---

## Cron with Gateway

When the gateway is running, cron job results can be sent to a messaging platform:

```yaml
gateway:
  telegram:
    home_channel: "-100123456789"  # results sent here
```

Set the home channel from the TUI:

```
/sethome
```

---

## Timezone

Cron schedules respect the `timezone` config key:

```yaml
timezone: "America/New_York"
```

Override with env var:

```bash
EDGECRAB_TIMEZONE=Europe/London edgecrab cron list
```

---

## The `/cron` Slash Command

From the TUI:

```
/cron                # open cron job manager UI
```

Shows all jobs with their next run time, status, and last run output.

---

## Practical Cron Job Examples

**Daily standup summary (weekdays 9 AM):**
```bash
edgecrab cron add --name standup "0 9 * * 1-5" \
  "Check GitHub notifications, open PRs, and failing CI. Summarize in 5 bullet points."
```

**Weekly code quality check (Sunday midnight):**
```bash
edgecrab cron add --name weekly-quality "0 0 * * 0" \
  "Run cargo clippy --workspace 2>&1, cargo test --workspace 2>&1. Report any new warnings or failures."
```

**Hourly log watcher:**
```bash
edgecrab cron add --name log-watch "0 * * * *" \
  "Check the last hour of /var/log/app.log for ERROR or CRITICAL entries. Alert if found."
```

---

## Pro Tips

**Send results to Telegram.** Set `gateway.telegram.home_channel` and run the gateway — cron job results arrive as Telegram messages. Perfect for monitoring tasks you want to know about without watching a terminal.

**Test before scheduling.** Run the task once manually to verify it works:
```bash
edgecrab "Summarize overnight GitHub notifications and Slack messages"
```
Once the output looks right, add it as a cron job.

**Use specific toolsets per job.** If a cron job only needs web access, restrict it:
```json
{ "name": "news-summary", "schedule": "0 7 * * *", "task": "...", "toolsets": ["web"] }
```
This prevents a cron job from accidentally modifying files.

---

## Frequently Asked Questions

**Q: Cron jobs aren't running. What's happening?**

Cron jobs only run while EdgeCrab is running. The cron scheduler is embedded — no system cron (`crontab`) is involved. Keep EdgeCrab running in a background session or Docker container for scheduled jobs to fire.

**Q: How do I see the output of the last cron run?**

```bash
edgecrab cron show morning-standup   # last output + next run time
```
Or check the logs via `edgecrab gateway logs` if the gateway is running.

**Q: Can a cron job trigger a gateway message?**

Yes. Set `gateway.telegram.home_channel` (or Discord/Slack equivalent). The cron job's final response is sent to that channel automatically.

**Q: What happens if a cron job is still running when the next trigger fires?**

The new run is skipped with a warning. Cron jobs do not run concurrently with themselves.

**Q: How do I pause all cron jobs temporarily?**

```bash
edgecrab cron disable --all    # disable all
edgecrab cron enable --all     # re-enable all
```

---

## See Also

- [Gateway / Messaging](/user-guide/messaging/) — Send cron results to external platforms
- [Configuration](/user-guide/configuration/) — `timezone`, cron defaults
- [CLI Commands](/reference/cli-commands/) — Full `edgecrab cron` subcommand reference

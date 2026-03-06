# memum-activity-bot

A compact reference describing the bot's slash commands and the background sweeper behavior.

## Commands

### Registration
- `/register <minecraft_username>`
  - Link a Discord user to a Minecraft account.
  - Verifies ownership via Hypixel social links, assigns the guild's configured registered role, seeds baseline stat snapshots, and begins XP tracking.
- `/unregister`
  - Remove the user's registration, delete DB row, and remove the registered role (if configured).
- `/send_registration_message <channel>` (admin-only)
  - Posts a persistent registration message with a "Register" button that runs the same registration flow when clicked.

### Stats & Level
- `/stats [user]`
  - Shows configured stats as change since `/register` (latest snapshot − baseline).
- `/level [user]`
  - Shows XP, computed level, progress toward next level, and recent stat deltas.
  - Sends a generated PNG "level card" image.

### Admin
- `/set-register-role <role>` (admin-only)
  - Configure the role assigned to users on successful registration.
- `/edit-stats ...` (admin-only) — parent command with subcommands:
  - `add-bedwars <mode> <metric> <xp_per_unit>` (`add-bedwars`)
    - Add a Bedwars stat (mode + metric) to the guild XP configuration.
  - `add-discord <stat> <xp_per_unit>` (`add-discord`)
    - Add a Discord activity stat (messages, reactions, commands, ...).
  - `edit <stat_name> <new_xp_value>` (`edit`)
    - Change XP/unit for an existing configured stat.
  - `remove <stat_name>`
    - Remove a stat from the XP configuration (snapshots preserved).
  - `list`
    - Show currently configured stats and XP/unit values.

Notes:
- Admin commands are restricted to user IDs configured in the app `admin_user_ids` list.
- Autocomplete is provided where appropriate (e.g. bedwars modes/metrics, tracked discord stats, configured stats).

### Leaderboard
- `/leaderboard`
  - Generate a paginated leaderboard image showing top players by total XP. Uses buttons for pagination and a timed cache.
- `/leaderboard_create <channel>` (admin-only)
  - Create a persistent leaderboard: sends one message per page and stores message IDs for automatic background updates.
- `/leaderboard_remove` (admin-only)
  - Remove the persistent leaderboard and delete stored messages / DB entry.

## Background Sweeper (summary)

There are two repeating background sweepers that keep snapshots up to date and convert stat changes into XP:

- Hypixel sweeper
  - Interval: slower (configured via `hypixel_sweep_interval_seconds`).
  - For every registered user:
    - Fetches Bedwars stats from Hypixel once per sweep.
    - Inserts a new snapshot for each tracked Hypixel stat.
    - Computes deltas against the previous snapshot and produces `StatDelta` records.
    - Passes deltas into the shared XP pipeline which atomically updates the user's XP and level.

- Discord sweeper
  - Interval: faster (configured via `discord_sweep_interval_seconds`).
  - For every registered user:
    - Reads latest Discord activity snapshots (messages_sent, reactions_added, commands_used, etc.).
    - Uses per-stat sweep cursors to only award XP for new activity since the last processed snapshot.
    - If a cursor is missing, it bootstraps the cursor from the snapshot value at or before the user's last XP timestamp (avoids retroactive XP spikes).
    - Cursor updates are stored atomically alongside any XP changes.

Shared details
- XP calculation uses the guild's `xp_config` mapping (stat_key → XP per unit).
- XP updates are applied inside a DB transaction to keep them atomic and to update level if thresholds are crossed.
- Snapshots are kept in the DB so history can be inspected and leaderboards/level cards can be generated.

## Notes & Tips
- Make sure a guild has a `registered_role` configured before users try to register.
- Admins manage which stats are tracked and how much XP they award via `/edit-stats`.
- Persistent leaderboard pages are updated by a background updater; `/leaderboard_create` stores message IDs for that purpose.

If you need this condensed into an in-client help message or want quick example invocations for your guild (with exact names/aliases), tell me which commands you want example usage for and I'll add them.

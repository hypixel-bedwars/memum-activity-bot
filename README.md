# Memum Activity Bot

A Discord bot for Minecraft guilds, focused on tracking player stats, XP, and milestones, with rich leaderboard and registration features. Built with Rust and [Poise](https://github.com/serenity-rs/poise).

---

## Features

- **User Registration**: Link Discord and Minecraft accounts.
- **Stat Tracking**: Track Bedwars and Discord activity stats.
- **XP & Level System**: Earn XP for in-game and Discord activity.
- **Leaderboards**: Paginated, image-based leaderboards with persistent channel support.
- **Milestones**: Custom level milestones with live progress.
- **Admin Controls**: Fine-grained XP/stat configuration, role management, and more.

---

## Commands

### Registration

| Command                                | Description                                                      | Permissions |
| -------------------------------------- | ---------------------------------------------------------------- | ----------- |
| `/send_registration_message <channel>` | Post a persistent registration message with a "Register" button. | Admin       |

### Stats & Levels

| Command  | Description                                                                                      | Permissions |
| -------- | ------------------------------------------------------------------------------------------------ | ----------- |
| `/stats` | Show your stat changes since registration and XP rewards for each stat.(Level command is better) | User        |
| `/level` | Show your XP, level, progress to next level, and a level card image.                             | User        |

### Leaderboard

| Command                         | Description                                                     | Permissions |
| ------------------------------- | --------------------------------------------------------------- | ----------- |
| `/leaderboard`                  | Show a paginated leaderboard image of top players in the guild. | User        |
| `/leaderboard-create <channel>` | Set up a persistent leaderboard in a channel (auto-updating).   | Admin       |
| `/leaderboard-remove`           | Remove the persistent leaderboard and stop auto-updates.        | Admin       |

### Milestones

| Command                     | Description                                                                     | Permissions |
| --------------------------- | ------------------------------------------------------------------------------- | ----------- |
| `/milestone add <level>`    | Add a new milestone level. Appears on the leaderboard.                          | Admin       |
| `/milestone edit <level>`   | Edit an existing milestone's level.                                             | Admin       |
| `/milestone remove <level>` | Remove a milestone.                                                             | Admin       |
| `/milestone view`           | Show your progress toward the next milestone.(level command shows this as well) | User        |

### Events

| Command              | Description                                                                             | Permissions |
| -------------------- | --------------------------------------------------------------------------------------- | ----------- |
| `/event list`        | List all events in the guild, including their status, start, and end dates.             | User        |
| `/event info <name>` | Show detailed information about a specific event, including its description and status. | User        |
| `/event leaderboard` | Display the leaderboard for a specific event, with pagination support.                  | User        |
| `/event level`       | Show your stats and rank for a specific event, with a level card image.                 | User        |
| `/event statistics`  | Show aggregated statistics for a specific event as a card image.                        | User        |
| `/event milestones`  | Show milestone completers for an event as a Discord embed.                              | User        |

### Admin: General Configuration

| Command                   | Description                                                                | Permissions |
| ------------------------- | -------------------------------------------------------------------------- | ----------- |
| `/audit-users <fix=bool>` | Checks for users that have left (It runs autocallically in the background) | Admin       |

### Admin: Stat & XP Configuration

| Command                                        | Description                               | Permissions |
| ---------------------------------------------- | ----------------------------------------- | ----------- |
| `/edit-stats add-bedwars <mode> <metric> <xp>` | Add a Bedwars stat to XP config.          | Admin       |
| `/edit-stats add-discord <stat> <xp>`          | Add a Discord activity stat to XP config. | Admin       |
| `/edit-stats edit <stat> <xp>`                 | Edit XP value for a configured stat.      | Admin       |
| `/edit-stats remove <stat>`                    | Remove a stat from XP config.             | Admin       |
| `/edit-stats list`                             | List all stats in XP config.              | Admin       |

### Admin: Role Management

| Command                                  | Description                                                         | Permissions |
| ---------------------------------------- | ------------------------------------------------------------------- | ----------- |
| `/set-register-role <role>`              | Set the role assigned to users on registration.                     | Admin       |
| `/set-nickname-registration-role <role>` | Allow members with this role to auto-register via nickname parsing. | Admin       |
| `/clear-nickname-registration-role`      | Require all users to use `/register`.                               | Admin       |

### Admin: XP Management

| Command                       | Description            | Permissions |
| ----------------------------- | ---------------------- | ----------- |
| `/xp add <@user> <amount>`    | Add XP to a user.      | Admin       |
| `/xp remove <@user> <amount>` | Remove XP from a user. | Admin       |

### Admin: Registration Override

| Command                                        | Description                                                       | Permissions |
| ---------------------------------------------- | ----------------------------------------------------------------- | ----------- |
| `/force-register <@user> <minecraft_username>` | Forcibly register a user, bypassing Hypixel Discord verification. | Admin       |
| `/force-unreister <@user>`                     | Forcibly unregister a user                                        | Admin       |

## Admin: Event Managment

| Command                                | Description                                                                          | Permissions |
| -------------------------------------- | ------------------------------------------------------------------------------------ | ----------- |
| `/edit-event new <name> <start> <end>` | Create a new event with a name, start date, and end date. (Optionally a description) | Admin       |
| `/edit-event edit <name>`              | Edit an existing event's name, description, or dates.                                | Admin       |
| `/edit-event delete <name>`            | Delete a pending or active event.                                                    | Admin       |
| `/edit-event start <name>`             | Force start an event.                                                                | Admin       |
| `/edit-event end <name>`               | Force end an active event.                                                           | Admin       |
| `/edit-event participants <name>`      | List event participants with pagination.                                             | Admin       |
| `/edit-event stats-add <name>`         | Add a tracked stat to an event.                                                      | Admin       |
| `/edit-event stats-remove <name>`      | Remove a tracked stat from an event.                                                 | Admin       |
| `/edit-event stats-edit <name>`        | Change the XP-per-unit for a stat in an event.                                       | Admin       |
| `/edit-event list`                     | List all events and their status.                                                    | Admin       |
| `/edit-event backfill <name>`          | Manually trigger a retroactive XP backfill for an event.                             | Admin       |
| `/edit-event leaderboard persist`      | Send a persistent leaderboard message for an event.                                  | Admin       |
| `/edit-event leaderboard-remove`       | Remove a persistent event leaderboard and delete its Discord messages.               | Admin       |
| `/edit-event status create`            | Create a persistent status message for an event.                                     | Admin       |
| `/edit-event status remove`            | Remove the persistent status message for an event.                                   | Admin       |
| `/edit-event milestones-add <name>`    | Add XP-threshold milestones to an event.                                             | Admin       |
| `/edit-event milestones-remove <name>` | Remove XP-threshold milestones from an event.                                        | Admin       |

---

### Command Structure

All commands are implemented as Discord slash commands using [Poise](https://github.com/serenity-rs/poise). Each command is annotated with `#[poise::command(...)]` and includes detailed Rust doc comments in the source. All admin command need ADMINSISTRATOR permission to execute them.

#### Example Command Handler

```rust
/// Register your Minecraft account to start tracking stats and earning XP.
/// Using required_permissions = "ADMINISTRATOR" for permission check
#[poise::command(slash_command, guild_only)]
pub async fn register(ctx: Context<'_>, minecraft_username: String) -> Result<(), Error> {
    // ...
}
```

### Stat Configuration

- **Bedwars Stats**: Configurable by mode and metric (e.g., `eight_two_final_kills_bedwars`).
- **Discord Stats**: Configurable for tracked activities (messages, reactions, etc.).
- **XP Values**: Set per-stat via `/edit-stats` commands.

### Milestones

- **Add/Edit/Remove**: Managed via `/milestone` subcommands.
- **Progress**: Displayed to users with `/milestone view` and on the leaderboard.

### Persistent Leaderboard

- **Setup**: `/leaderboard_create <channel>`
- **Remove**: `/leaderboard_remove`
- **Auto-Update**: Messages are updated automatically by the bot.

### Registration Flow

1. Users with the verfied role (which can be set) can press the button on the reg message.
2. Bot verifies ownership via Mojang and Hypixel APIs.
3. On success, user is assigned the configured registered role.
4. Admins can override with `/force_register` and also `/force_unregister`.

---

## Development

### Project Structure

- `src/commands/` — All command handlers, grouped by feature.
- `src/cards/` — Image generation for level and leaderboard cards.
- `src/database/` — Database models and queries.
- `src/discord_stats/` — Discord activity tracking.
- `src/hypixel/` — Hypixel API integration.
- `src/sweeper` — Background tasks for XP tracking, leaderboard updates, and user audits.
- `src/utils/` — Shared utilities.
- `src/fonts/` — Font files for image generation.
- `migrations/` — SQL migration scripts for database schema.

### Running the Bot

1. Set up environment variables. An example [.env](.env.example) file is provided
2. Build and run with Cargo, cargo also takes care of the migration scripts:

```bash
cargo run --release
```

3. After running the bot, add the verfied role using `set-nickname-registration-role` command, and then set the registration role using `/set_register_role` command.
4. Send the registration embed using `/send_registration_message` command. The embed comes with a button and some instructions for users to register their Minecraft accounts.
5. You can add milestones, edit stats, manage events, and make the bot your own. Have fun!!!

---

## Contributing

- See the source code for detailed documentation on each command and module.
- Contributions and issues are welcome!

---

## License

MIT

/// `/send_registration_message` command — admin only.
///
/// Posts a persistent registration message containing instructions and a
/// "Register" button in the specified channel. When a member clicks the
/// button, the bot reads their server nickname, extracts their Minecraft
/// username, and runs the same registration flow as `/register`.
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::info;

use crate::shared::types::{Context, Error};

/// Send a registration message with a Register button to a channel. Admin only.
#[poise::command(slash_command, guild_only, ephemeral)]
pub async fn send_registration_message(
    ctx: Context<'_>,
    #[description = "The channel to send the registration message to"]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    if !ctx
        .data()
        .config
        .admin_user_ids
        .contains(&ctx.author().id.get())
    {
        let embed = CreateEmbed::default()
            .title("Permission Denied")
            .color(0xFF4444)
            .description("You do not have permission to use this command.");
        ctx.send(poise::CreateReply::default().embed(embed)).await?;
        return Ok(());
    }

    let instructions = "\
        ## How to Register\n\
        To link your Minecraft account and start earning XP, press the **Register** button below.\n\n\
        **Before you press it, make sure:**\n\
        - Your server nickname follows the format: `[NNN emoji] YourMinecraftUsername`\n\
          *(e.g. `[313 💫] VA80`, `[204 ✨] CosmicFuji`)*\n\
        - Your **Hypixel** profile has your Discord tag set as a social link\n\
          *(in-game: `/profile` → Social Media → Discord)*\n\n\
        Once both are set, hit the button and the bot will handle the rest.";

    let message = serenity::CreateMessage::new()
        .content(instructions)
        .components(vec![serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new("register_button")
                .label("Register")
                .style(serenity::ButtonStyle::Success),
        ])]);

    channel
        .id
        .send_message(&ctx.serenity_context().http, message)
        .await?;

    info!(
        admin = ctx.author().id.get(),
        channel = channel.id.get(),
        "Registration message sent."
    );

    let embed = CreateEmbed::default()
        .title("Registration Message Sent")
        .color(0x00BFFF)
        .description(format!("Registration message sent to <#{}>.", channel.id));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

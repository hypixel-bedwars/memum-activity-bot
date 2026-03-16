/// `/send_registration_message` command — admin only.
///
/// Posts a persistent registration message containing instructions and a
/// "Register" button in the specified channel. When a member clicks the
/// button, the bot will verify their already-registered Minecraft account
/// and link it to the server.
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use tracing::info;

use crate::shared::types::{Context, Error};

/// Send a registration message with a Register button to a channel. Admin only.
#[poise::command(
    slash_command,
    guild_only,
    ephemeral,
    check = "crate::utils::permissions::admin_check"
)]
pub async fn send_registration_message(
    ctx: Context<'_>,
    #[description = "The channel to send the registration message to"]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    let embed = CreateEmbed::default()
        .title("🔗 Account Registration")
        .color(0x00BFFF)
        .description(
            "Link your **Minecraft account** to start earning **XP** and tracking your stats on this server."
        )
        .field(
            "📝 Step 1 — Link Your Discord in Hypixel",
            "If yoo have already linked your account you can skip this\n\
             If you have not linked your account follow this [video guide](https://youtu.be/UresIQdoQHk?si=vwo1WoeSdWP2xPE9) ",
            false,
        )
        .field(
            "✅ Final Step",
            "Once that step is completed, press the **Register** button below.\n\
            The bot will verify your account and finish the registration process.",
            false,
        )
        .footer(serenity::CreateEmbedFooter::new(
            "Please make sure you have a nickname that has not been modified",
        ));

    let message = serenity::CreateMessage::new().embed(embed).components(vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new("register_button")
                .label("Register")
                .emoji('✅')
                .style(serenity::ButtonStyle::Success),
        ]),
    ]);

    channel
        .id
        .send_message(&ctx.serenity_context().http, message)
        .await?;

    info!(
        "Sent registration message to channel '{}' ({})",
        channel.name, channel.id
    );

    let embed = CreateEmbed::default()
        .title("Registration Message Sent")
        .color(0x00BFFF)
        .description(format!("Registration message sent to <#{}>.", channel.id));
    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

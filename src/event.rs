use std::time::{SystemTime, UNIX_EPOCH};

use poise::serenity_prelude::{self as serenity, ComponentInteraction, CreateInteractionResponse, CreateInteractionResponseMessage};
use crate::{Data, Error};

#[derive(Debug, sqlx::FromRow)]
struct Invite{
    id: i32,
    user_id: String,
    unix: i64,
    msg_id: String,
    invite: String,
}

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error>{
    match event {
        serenity::FullEvent::InteractionCreate{
            interaction: serenity::Interaction::Component(component)
        } => {
            let id = component.data.custom_id.clone().split('-').map(|id| id.to_string()).collect::<Vec<_>>();
            let uid = id[0].clone();
            let timestamp = id[1].clone();
            match id.len(){
                2 => get_invite_link(ctx, component, data, &uid, &timestamp).await?,
                3 => delete(ctx, component).await?,
                _ => {
                    return Ok(());
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn get_invite_link(
    ctx: &serenity::Context,
    mci: &ComponentInteraction,
    data: &Data,
    uid: &str,
    timestamp: &str,
) -> Result<(), Error> {
  
    let unix = timestamp.parse::<i64>()?;
    let invite: Invite = sqlx::query_as!(Invite,"
        SELECT id, user_id, unix, msg_id, invite
        FROM invitations
        WHERE user_id = $1 AND unix = $2;",
        uid.to_string(),
        unix)
    .fetch_one(&data.pool)
    .await?;
    let rsvpuid = mci.user.id.to_string();
    let rsvpnow = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;
    let rsvp = sqlx::query!(r#"
        INSERT INTO rsvps (user_id, invitation_id, unix)
        VALUES ($1, $2, $3)
    "#, rsvpuid, invite.id, rsvpnow)
        .execute(&data.pool)
        .await?;
    let resp = {
        let msg = CreateInteractionResponseMessage::new()
            .ephemeral(true)
            .content(format!("Invite link: {}", invite.invite.clone()));
        CreateInteractionResponse::Message(msg)
    };
    mci.create_response(ctx, resp).await?;
    Ok(())
}

async fn delete(
    ctx: &serenity::Context,
    mci: &ComponentInteraction,
) -> Result<(), Error> {
    let msgid = mci.message.id;
    let author = mci.message.author.id;
    let btnpresser = mci.user.id;
    if author.eq(&btnpresser){
        mci.channel_id.delete_message(ctx, msgid).await?;
    } else{
        mci.create_response(ctx, CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .ephemeral(true)
                .content("You are not the author of this message!"),
        )).await?;
    }
    Ok(())
}
use event::event_handler;
use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, CreateActionRow, CreateAttachment, CreateButton, CreateEmbed,
        CreateEmbedAuthor, CreateMessage,
    },
};
use regex::Regex;
use sqlx::{postgres::PgPoolOptions, Executor, PgPool, Pool, Postgres};
use std::time::{SystemTime, UNIX_EPOCH};
mod event;
struct Data {
    pool: Pool<Postgres>,
}
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

fn geturl(input: &str) -> Option<String> {
    let link_re =
        Regex::new(r"^https://link\.brawlstars\.com/invite/gameroom/[a-z]{2}\?tag=[A-Z0-9]+$")
            .unwrap();
    let code_re = Regex::new(r"^[A-Z0-9]{6,10}$").unwrap();
    if input.starts_with("https://link.brawlstars.com") && link_re.is_match(input) {
        return Some(input.to_string());
    } else if code_re.is_match(input) {
        return Some(format!(
            "https://link.brawlstars.com/invite/gameroom/en?tag={}",
            input
        ));
    } else {
        return None;
    }
}
/// Send an invitation link to a room
#[poise::command(slash_command, guild_only)]
async fn send(
    ctx: Context<'_>,
    #[description = "Room link or code"] url: String,
    #[description = "Additional room information. Please don't include the room link here"] content: Option<String>,
) -> Result<(), Error> {
    let url = match geturl(&url) {
        Some(url) => url,
        None => {
            ctx.send(CreateReply::default().ephemeral(true).content("Invalid room code or room link!"))
                .await?;
            return Ok(());
        }
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
    let uid = ctx.author().id;
    let msg = {
        let invid = format!("{}-{}", uid, now);
        let delid = format!("{}-{}-del", uid, now);
        let bottomline = "**Press the button below to get the room link**";
        let invbtn = CreateButton::new(invid.clone())
            .style(serenity::ButtonStyle::Primary)
            .label("Get room link");
        let delbtn = CreateButton::new(delid)
            .style(serenity::ButtonStyle::Danger)
            .label("Delete message");
        let embed = CreateEmbed::new()
            .author(
                CreateEmbedAuthor::new(ctx.author().name.clone())
                    .icon_url(ctx.author().avatar_url().unwrap_or_default()),
            )
            .title("Room link")
            .description(format!(
                "{}\n{}",
                content.unwrap_or("No content".to_string()),
                bottomline
            ));
        CreateMessage::new()
            .embed(embed)
            .components(vec![CreateActionRow::Buttons(vec![invbtn, delbtn])])
    };

    let cid = ctx.channel_id();
    let msg_id = cid.send_message(ctx, msg).await?.id;
    let query = sqlx::query!(
        "INSERT INTO invitations (user_id, unix, msg_id, invite) VALUES ($1, $2, $3, $4)",
        uid.get().to_string(),
        now as i64,
        msg_id.get().to_string(),
        url
    );
    ctx.data().pool.execute(query).await?;
    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content("We sent your message!"),
    )
    .await?;

    Ok(())
}

async fn check(ctx: Context<'_>) -> Result<bool, Error> {
    if let Some(perms) = ctx.author_member().await.and_then(|m| m.permissions) {
        return Ok(perms.ban_members());
    }
    Ok(false)
}

struct RSVP {
    user_id: String,
    unix: i64,
}

#[poise::command(guild_only, slash_command, check = "check")]
async fn inspect(
    ctx: Context<'_>,
    #[description = "Message ID"] msg_id: String,
) -> Result<(), Error> {
    let rsvps: Vec<RSVP> = sqlx::query_as!(
        RSVP,
        r#"
        SELECT r.user_id, r.unix
        FROM rsvps AS r
        INNER JOIN invitations AS i
        ON r.invitation_id = i.id
        WHERE i.msg_id = $1
    "#,
        msg_id
    )
    .fetch_all(&ctx.data().pool)
    .await?;

    let mut csv = String::new();
    csv.push_str("user_id,unix\n");
    for rsvp in rsvps {
        csv.push_str(&format!("{},{}\n", rsvp.user_id, rsvp.unix));
    }
    let csv_file = serenity::CreateAttachment::bytes(csv, format!("rsvps-{}.csv", msg_id));
    let msg = CreateReply::default()
        .content("Here are the RSVPs")
        .reply(true)
        .attachment(csv_file);
    ctx.send(msg).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // #[cfg(debug_assertions)]
    // dotenv::dotenv().ok();
    let token = std::env::var("BOT_TOKEN").expect("missing DISCORD_TOKEN");
    let db = connect().await?;
    let intents = serenity::GatewayIntents::non_privileged();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![send(), inspect()],
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data { pool: db.clone() })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;

    client.unwrap().start().await?;
    Ok(())
}

pub async fn connect() -> Result<Pool<Postgres>, Error> {
    let db_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            println!("Missing DATABASE_URL");
            return Err("Missing DATABASE_URL".into());
        }
    };
    let pool = PgPool::connect(&db_url).await?;
    Ok(pool)
}

use event::event_handler;
use poise::{
    CreateReply,
    serenity_prelude::{
        self as serenity, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedAuthor,
        CreateMessage, Mentionable, Timestamp,
    },
};
use regex::Regex;
use sqlx::{Executor, PgPool, Pool, Postgres};
use std::fmt::Display;
use std::str::FromStr;

mod event;
use redis::{AsyncCommands, aio::MultiplexedConnection};
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

struct Data {
    pool: Pool<Postgres>,
    redis: MultiplexedConnection,
}

struct RedisKey{
    uid: String,
    cid: String,
}

impl Display for RedisKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.uid, self.cid)
    }
}

impl FromStr for RedisKey {
    type Err = String; 
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err("Invalid format: Expected 'uid-cid'".to_string());
        }
        Ok(RedisKey {
            uid: parts[0].to_string(),
            cid: parts[1].to_string(),
        })
    }
}


fn geturl(input: &str) -> Option<String> {
    let link_re =
        Regex::new(r"^https://link\.brawlstars\.com/invite/gameroom/[a-z]{2}\?tag=[A-Za-z0-9]+$")
            .unwrap();
    let code_re = Regex::new(r"^[A-Za-z0-9]{6,10}$").unwrap();
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
    use std::time::{SystemTime, UNIX_EPOCH};

    let uid = ctx.author().id;
    let cid = ctx.channel_id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    let slowmode = ctx
        .guild_channel()
        .await
        .and_then(|c| c.rate_limit_per_user)
        .unwrap_or(0);
    let redis = ctx.data().redis.clone();
    if slowmode > 0{
        let key = RedisKey {
            uid: uid.get().to_string(),
            cid: cid.get().to_string(),
        };
        if let Ok(Some(_)) = redis.clone().get::<_, Option<String>>(key.to_string()).await {
            ctx.send(
                CreateReply::default()
                    .ephemeral(true)
                    .content("You are on cooldown! Please wait a bit before sending another message."),
            )
            .await?;
            return Ok(());
        }
    }

    let url = match geturl(&url) {
        Some(u) => u,
        None => {
            ctx.send(CreateReply::default().ephemeral(true).content(
                "Invalid room code or room link! Please make sure it's a valid code or link.",
            ))
            .await?;
            return Ok(());
        }
    };

    let invid = format!("{}-{}", uid, now);
    let delid = format!("{}-{}-del", uid, now);
    let bottomline = "Press the button below to get the room link";

    let invbtn = CreateButton::new(invid.clone())
        .style(serenity::ButtonStyle::Primary)
        .label("Get room link");

    let delbtn = CreateButton::new(delid)
        .style(serenity::ButtonStyle::Danger)
        .label("Delete message");
    let author_name = ctx.author().name.clone();
    let embed = CreateEmbed::new()
        .author(
            CreateEmbedAuthor::new(author_name.clone())
                .icon_url(ctx.author().avatar_url().unwrap_or_default()),
        )
        .title(format!("{} is looking for more players!", author_name))
        .timestamp(Timestamp::now())
        .description(format!(
            r#"From: {}
Room description: {}
-# {}"#,
            ctx.author().mention(),
            content.unwrap_or("_The host does not disclose anything_".to_string()),
            bottomline
        ));

    let message = CreateMessage::new()
        .embed(embed)
        .components(vec![CreateActionRow::Buttons(vec![invbtn, delbtn])]);

    let sent_msg = cid.send_message(ctx, message).await?;

    let query = sqlx::query!(
        "INSERT INTO invitations (user_id, unix, msg_id, invite) VALUES ($1, $2, $3, $4)",
        uid.get().to_string(),
        now as i64,
        sent_msg.id.get().to_string(),
        url
    );
    ctx.data().pool.execute(query).await?;
    let redis_key = RedisKey {
        uid: uid.get().to_string(),
        cid: cid.get().to_string(),
    };
    if slowmode > 0 {

        redis
            .clone()
            .set_ex::<String, u64, u64>(
                redis_key.to_string(),
                slowmode as u64,
                slowmode as u64,
            )
            .await.ok();
    }
    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content("We sent your invitation to the chat!"),
    )
    .await?;

    Ok(())
}

async fn check(ctx: Context<'_>) -> Result<bool, Error> {
    if let Some(perms) = ctx.author_member().await.and_then(|m| m.permissions) {
        return Ok(perms.ban_members());
    }
    ctx.send(
        CreateReply::default()
            .ephemeral(true)
            .content("You don't have permission to use this command!"),
    )
    .await?;
    Ok(false)
}

struct RSVP {
    user_id: String,
    unix: i64,
}
/// [Mod only] Inspect RSVPs for a message
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
        .ephemeral(true)
        .attachment(csv_file);
    ctx.send(msg).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // #[cfg(debug_assertions)]
    // dotenv::dotenv().ok();
    let token = std::env::var("BOT_TOKEN").expect("missing DISCORD_TOKEN");
    let data = connect().await?;
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
                Ok(data)
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;

    client.unwrap().start().await?;
    Ok(())
}

async fn connect() -> Result<Data, Error> {
    let db_url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            println!("Missing DATABASE_URL");
            return Err("Missing DATABASE_URL".into());
        }
    };
    let redis_url = match std::env::var("REDIS_URL") {
        Ok(url) => url,
        Err(_) => {
            println!("Missing REDIS_URL");
            return Err("Missing REDIS_URL".into());
        }
    };
    let pool = PgPool::connect(&db_url).await?;
    let redis_client = redis::Client::open(redis_url)?;
    let redis = redis_client.get_multiplexed_async_connection().await?;
    Ok(Data { pool, redis })
}

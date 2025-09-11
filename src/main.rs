use std::env;
use std::path::Path;
use std::path::PathBuf;

use serenity::all::Attachment;
use serenity::all::CommandId;
use serenity::all::CreateAttachment;
use serenity::all::CreateInteractionResponse;
use serenity::all::CreateInteractionResponseFollowup;
use serenity::all::CreateInteractionResponseMessage;
use serenity::all::CreateMessage;
use serenity::all::InteractionContext;
use serenity::async_trait;
use serenity::builder::CreateCommand;
use serenity::model::application::{Command, Interaction};
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use tokio::io::AsyncWriteExt;
use tokio::process::Command as TokioCommand;

struct Handler {
    mesh_thumbnail_path: String,
    gifski_path: String,
    frames_per_second: f32,
    frames: u32,
    delete_old_interactions: bool,
}

async fn generate_gif_from_attachment(
    attachment: &Attachment,
    settings: &Handler,
) -> Option<CreateAttachment> {
    let temp_dir = tempfile::tempdir().expect("Failed to create temporary directory");

    let original_filename = PathBuf::from(&attachment.filename);
    let original_base_filename = original_filename
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let extension = match original_filename.extension() {
        Some(ext) => ext.to_string_lossy().to_string().to_lowercase(),
        None => {
            println!(
                "Attachment {} has no valid extension, skipping.",
                attachment.filename
            );
            return None;
        }
    };

    let file_path = temp_dir.path().join(format!("a.{}", extension));
    let content = match attachment.download().await {
        Ok(content) => content,
        Err(e) => {
            println!(
                "Failed to download attachment {}: {}",
                attachment.filename, e
            );
            return None;
        }
    };

    let mut file = tokio::fs::File::create(&file_path)
        .await
        .expect("Failed to create file");

    if let Err(why) = file.write_all(&content).await {
        println!("Failed to write to file {:?}: {}", file_path, why);
        return None;
    }

    let mut mesh_thumbnail_command = TokioCommand::new(&settings.mesh_thumbnail_path);

    mesh_thumbnail_command
        .arg(file_path.to_str().unwrap())
        .arg("--outdir")
        .arg(temp_dir.path())
        .arg("--images-per-file")
        .arg(settings.frames.to_string())
        .arg("--rotatey")
        .arg("35")
        .arg("--inverse-zoom")
        .arg("1.25")
        .arg("--color")
        .arg("FFFFFF");

    if let Err(why) = mesh_thumbnail_command.status().await {
        println!("Failed to execute mesh thumbnail command: {}", why);
        return None;
    }

    let mut gif_path = PathBuf::new();
    gif_path.push(temp_dir.path());
    gif_path.push(format!("{}.gif", uuid::Uuid::new_v4()));

    let mut gifski_command = TokioCommand::new(&settings.gifski_path);

    gifski_command
        .current_dir(temp_dir.path())
        .arg("-o")
        .arg(gif_path.to_str().unwrap())
        .arg("--fps")
        .arg(settings.frames_per_second.to_string())
        .args((0..settings.frames).map(|i| format!("a-{:02}.png", i)));

    if let Err(why) = gifski_command.status().await {
        println!("Failed to execute gifski command: {}", why);
        return None;
    }

    let file = match tokio::fs::File::open(&gif_path).await {
        Ok(file) => file,
        Err(e) => {
            println!("Failed to open GIF file {:?}: {}", &gif_path, e);
            return None;
        }
    };

    let attachment =
        match CreateAttachment::file(&file, format!("{}.gif", original_base_filename)).await {
            Ok(attachment) => attachment,
            Err(e) => {
                println!("Failed to create attachment for file {:?}: {}", &gif_path, e);
                return None;
            }
        };

    return Some(attachment);
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot || msg.guild_id.is_none() {
            return;
        }

        let filtered_attachments: Vec<Attachment> = msg
            .attachments
            .into_iter()
            .filter(|p| {
                vec![".3mf", ".stl", ".obj", ".gcode"]
                    .iter()
                    .any(|f| p.filename.to_lowercase().ends_with(f))
            })
            .collect();

        if filtered_attachments.is_empty() {
            return;
        }

        println!(
            "User {} ({}) sent a message with attachments:",
            msg.author.name, msg.author.id
        );

        filtered_attachments.iter().for_each(|f| {
            let filename = f.filename.clone();
            let content_type = f.content_type.clone().unwrap_or(String::from("unknown"));
            println!("Attachment: {} ({})", filename, content_type);
        });

        let typing = ctx.http.start_typing(msg.channel_id);

        for attachment in filtered_attachments {
            let attachment = match generate_gif_from_attachment(&attachment, &self).await 
            {
                Some(attachment) => attachment,
                None => continue
            };

            let filename = attachment.filename.clone();

            let message = CreateMessage::new()
                .add_file(attachment)
                .content(format!("{}", filename));

            if let Err(why) = msg.channel_id.send_message(&ctx.http, message).await {
                println!("Failed to send message: {}", why);
                continue;
            }
        }

        typing.stop();
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let command = match interaction {
            Interaction::Command(command) => command,
            _ => return,
        };

        if command.data.name != "Preview 3d model" {
            return;
        }

        let message = match command.data.resolved.messages.values().next() {
            Some(message) => message,
            None => return,
        };

        let filtered_attachments: Vec<Attachment> = message
            .attachments
            .iter()
            .filter(|p| {
                vec![".3mf", ".stl", ".obj", ".gcode"]
                    .iter()
                    .any(|f| p.filename.ends_with(f))
            })
            .cloned()
            .collect();

        if filtered_attachments.is_empty() {
            if let Err(why) = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .content("Failed to find any models in message"),
                    ),
                )
                .await
            {
                println!("Failed to reply to interaction: {}", why);
            }
            return;
        }

        println!(
            "User {} ({}) sent a message with attachments:",
            message.author.name, message.author.id
        );

        filtered_attachments.iter().for_each(|f| {
            let filename = f.filename.clone();
            let content_type = f.content_type.clone().unwrap_or(String::from("unknown"));
            println!("Attachment: {} ({})", filename, content_type);
        });

        if let Err(why) = command.defer(&ctx.http).await {
            println!("Failed to defer interaction: {}", why);
            return;
        }

        let mut gifs = vec![];

        for attachment in filtered_attachments {
            if let Some(message_attachment) = generate_gif_from_attachment(&attachment, &self).await {
                gifs.push(message_attachment);
            }
        }

        if gifs.is_empty() {
            println!("Failed to generate any GIFs from the attachments.");
            return;
        }

        if let Err(why) = command
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new().add_files(gifs),
            )
            .await
        {
            println!("Failed to send GIFs from the attachments: {}", why);
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        if self.delete_old_interactions {
            println!("Deleting old interactions...");
            let commands = Command::get_global_commands(&ctx.http)
                .await
                .unwrap();

            for command in &commands {
                Command::delete_global_command(&ctx.http, command.id).await.unwrap();
            }
            println!("Old interactions deleted.");
        }

        let global_create_command = Command::create_global_command(
            &ctx.http,
            CreateCommand::new("Preview 3d model")
                .kind(serenity::all::CommandType::Message)
                .add_context(InteractionContext::Guild)
                .add_context(InteractionContext::PrivateChannel)
                .add_context(InteractionContext::BotDm)
                .add_integration_type(serenity::all::InstallationContext::Guild)
                .add_integration_type(serenity::all::InstallationContext::User)
        )
        .await;

        if let Err(why) = global_create_command {
            println!("Failed to create global command: {}", why);
        } else {
            println!("Global command created successfully.");
        }
    }
}

#[tokio::main]
async fn main() {
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    let context = Handler {
        mesh_thumbnail_path: env::var("MESH_THUMBNAIL_PATH")
            .expect("Expected a mesh-thumbnail path in the environment"),
        gifski_path: env::var("GIFSKI_PATH").expect("Expected a gifski path in the environment"),
        frames_per_second: env::var("FRAMES_PER_SECOND")
            .unwrap_or_else(|_| "12.0".to_string())
            .parse()
            .expect("Expected a valid frames per second value"),
        frames: env::var("FRAMES")
            .unwrap_or_else(|_| "60".to_string())
            .parse()
            .expect("Expected a valid number of frames"),
        delete_old_interactions: env::var("DELETE_OLD_INTERACTIONS")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .expect("Expected a valid boolean for delete old interactions"),
    };

    if !Path::new(&context.mesh_thumbnail_path).is_file()
        || !Path::new(&context.gifski_path).is_file()
    {
        panic!("Invalid paths provided in the environment variables.");
    }

    let mut client = Client::builder(&token, intents)
        .event_handler(context)
        .await
        .expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}

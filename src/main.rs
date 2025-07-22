use std::env;
use std::path::PathBuf;
use std::path::Path;

use serenity::all::CreateAttachment;
use serenity::all::CreateMessage;
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use serenity::all::Attachment;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

struct Handler
{
    mesh_thumbnail_path: String,
    gifski_path: String,
    frames_per_second: f32,
    frames: u32,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot || msg.guild_id.is_none() {
            return;
        }

        let filtered_attachments : Vec<Attachment> = msg.attachments.into_iter().filter(|p| vec![".3mf", ".stl", ".obj", ".gcode"].iter().any(|f| p.filename.ends_with(f))).collect();

        if filtered_attachments.is_empty() {
            return;
        }

        println!("User {} ({}) sent a message with attachments:", msg.author.name, msg.author.id);

        filtered_attachments.iter().for_each(|f| {
            let filename = f.filename.clone();
            let content_type = f.content_type.clone().unwrap_or(String::from("unknown"));
            println!("Attachment: {} ({})", filename, content_type);
        });

        let typing = ctx.http.start_typing(msg.channel_id);

        for attachment in filtered_attachments {
            let temp_dir = tempfile::tempdir().expect("Failed to create temporary directory");

            let original_filename = PathBuf::from(&attachment.filename);
            let original_base_filename = original_filename.file_stem().unwrap().to_string_lossy().to_string();
            let extension = match original_filename.extension()
            {
                Some(ext) => ext.to_string_lossy().to_string(),
                None => {
                    println!("Attachment {} has no valid extension, skipping.", attachment.filename);
                    continue;
                },
            };

            let file_path = temp_dir.path().join(format!("a.{}", extension));
            let content = match attachment.download().await {
                Ok(content) => content,
                Err(e) => {
                    println!("Failed to download attachment {}: {}", attachment.filename, e);
                    continue;
                },
            };

            let mut file = tokio::fs::File::create(&file_path).await.expect("Failed to create file");

            if let Err(why) = file.write_all(&content).await {
                println!("Failed to write to file {:?}: {}", file_path, why);
                continue;
            }

            let mut mesh_thumbnail_command = Command::new(&self.mesh_thumbnail_path);
            
            mesh_thumbnail_command.arg(file_path.to_str().unwrap())
                .arg("--outdir")
                .arg(temp_dir.path())
                .arg("--images-per-file")
                .arg(self.frames.to_string())
                .arg("--rotatey")
                .arg("35")
                .arg("--inverse-zoom")
                .arg("1.25")
                .arg("--color")
                .arg("FFFFFF");

            if let Err(why) = mesh_thumbnail_command.status().await {
                println!("Failed to execute mesh thumbnail command: {}", why);
                continue;   
            }

            let mut gif_path = PathBuf::new();
            gif_path.push(temp_dir.path());
            gif_path.push(format!("{}.gif", uuid::Uuid::new_v4()));

            let mut gifski_command = Command::new(&self.gifski_path);

            gifski_command
                .current_dir(temp_dir.path())
                .arg("-o")
                .arg(gif_path.to_str().unwrap())
                .arg("--fps")
                .arg(self.frames_per_second.to_string())
                .args((0..self.frames).map(|i| format!("a-{:02}.png", i)));
            
            if let Err(why) = gifski_command.status().await {
                println!("Failed to execute gifski command: {}", why);
                continue;
            }

            let file = match tokio::fs::File::open(&gif_path).await {
                Ok(file) => file,
                Err(e) => {
                    println!("Failed to open GIF file {:?}: {}", gif_path, e);
                    continue;
                },
            };

            let attachment = match CreateAttachment::file(&file, format!("{}.gif", original_base_filename)).await {
                Ok(attachment) => attachment,
                Err(e) => {
                    println!("Failed to create attachment for file {:?}: {}", gif_path, e);
                    continue;
                },
            };

            let message = CreateMessage::new()
                .add_file(attachment)
                .content(format!("{}", original_base_filename));

            if let Err(why) = msg.channel_id.send_message(&ctx.http, message).await
            {
                println!("Failed to send message: {}", why);
                continue;
            }  
        }

        typing.stop();
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

#[tokio::main]
async fn main() {
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let context = Handler {
        mesh_thumbnail_path: env::var("MESH_THUMBNAIL_PATH").expect("Expected a mesh-thumbnail path in the environment"),
        gifski_path: env::var("GIFSKI_PATH").expect("Expected a gifski path in the environment"),
        frames_per_second: env::var("FRAMES_PER_SECOND").unwrap_or_else(|_| "12.0".to_string()).parse().expect("Expected a valid frames per second value"),
        frames: env::var("FRAMES").unwrap_or_else(|_| "60".to_string()).parse().expect("Expected a valid number of frames"),
    };

    if !Path::new(&context.mesh_thumbnail_path).is_file() 
        || !Path::new(&context.gifski_path).is_file() {
        panic!("Invalid paths provided in the environment variables.");
    }

    let mut client =
        Client::builder(&token, intents).event_handler(context).await.expect("Err creating client");

    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}

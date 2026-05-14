use crate::{
    bot::{Context, ensure_command_channel},
    settings::{ChannelFamily, HytalePasswordSettings},
};
use anyhow::Context as AnyhowContext;
use poise::serenity_prelude as serenity;
use serde::Deserialize;
use serde_json::{Map, Value};
use serenity::RoleId;
use std::{
    net::IpAddr,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::timeout,
};

type Error = anyhow::Error;

const DEFAULT_SERVICE_NAME: &str = "hytale-server.service";
const DEFAULT_COMMAND_TIMEOUT_SECONDS: u64 = 15;
const DEFAULT_DOWNLOAD_TIMEOUT_SECONDS: u64 = 1_800;
const DEFAULT_HYTALE_PORT: u16 = 5_520;
const MAX_RESPONSE_CHARS: usize = 1_750;

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum HytaleHelpTopicChoice {
    #[name = "overview"]
    Overview,
    #[name = "commands"]
    Commands,
    #[name = "settings"]
    Settings,
    #[name = "permissions"]
    Permissions,
    #[name = "operations flow"]
    OperationsFlow,
    #[name = "troubleshooting"]
    Troubleshooting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HytaleConfig {
    manager_role_id: RoleId,
    service_name: String,
    manage_script: PathBuf,
    hytale_dir: PathBuf,
    command_timeout: Duration,
    download_timeout: Duration,
}

impl HytaleConfig {
    fn from_env() -> Result<Self, ConfigError> {
        let manager_role_id = read_role_id("HYTALE_MANAGER_ROLE_ID")?;
        let service_name = read_env_string("HYTALE_SERVICE_NAME")
            .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_owned());
        let manage_script =
            read_env_path("HYTALE_MANAGE_SCRIPT").unwrap_or_else(default_manage_script);
        let hytale_dir = read_env_path("HYTALE_DIR").unwrap_or_else(default_hytale_dir);
        let command_timeout_seconds = read_u64(
            "HYTALE_COMMAND_TIMEOUT_SECONDS",
            DEFAULT_COMMAND_TIMEOUT_SECONDS,
        );
        let download_timeout_seconds = read_u64(
            "HYTALE_DOWNLOAD_TIMEOUT_SECONDS",
            DEFAULT_DOWNLOAD_TIMEOUT_SECONDS,
        );

        Ok(Self {
            manager_role_id,
            service_name,
            manage_script,
            hytale_dir,
            command_timeout: Duration::from_secs(command_timeout_seconds.max(1)),
            download_timeout: Duration::from_secs(download_timeout_seconds.max(1)),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfigError {
    MissingRole,
    InvalidRole(String),
}

impl ConfigError {
    fn setup_message(&self) -> String {
        match self {
            Self::MissingRole => {
                "Hytale controls are not set up yet. Ask the bot owner to set `HYTALE_MANAGER_ROLE_ID` to the Discord role allowed to manage the server.".to_owned()
            }
            Self::InvalidRole(value) => format!(
                "Hytale controls are paused because `HYTALE_MANAGER_ROLE_ID` is not a valid Discord role ID: `{}`.",
                truncate_inline(value, 80)
            ),
        }
    }
}

fn read_role_id(key: &str) -> Result<RoleId, ConfigError> {
    let value = std::env::var(key).map_err(|_| ConfigError::MissingRole)?;
    value
        .trim()
        .parse::<u64>()
        .map(RoleId::new)
        .map_err(|_| ConfigError::InvalidRole(value))
}

fn read_env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn read_env_path(key: &str) -> Option<PathBuf> {
    read_env_string(key).map(PathBuf::from)
}

fn read_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

fn default_manage_script() -> PathBuf {
    std::env::var("HOME")
        .map(|home| PathBuf::from(home).join("hytale/hytale-manage.sh"))
        .unwrap_or_else(|_| PathBuf::from("hytale-manage.sh"))
}

fn default_hytale_dir() -> PathBuf {
    std::env::var("HOME")
        .map(|home| PathBuf::from(home).join("hytale"))
        .unwrap_or_else(|_| PathBuf::from("hytale"))
}

#[poise::command(
    slash_command,
    subcommands(
        "help",
        "join",
        "status",
        "logs",
        "start",
        "stop",
        "restart",
        "check_update",
        "update",
        "set",
        "toggle"
    )
)]
pub async fn hytale(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "What to explain; defaults to overview"] topic: Option<HytaleHelpTopicChoice>,
) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Hytale).await? {
        return Ok(());
    }

    ctx.say(hytale_help_text(
        topic.unwrap_or(HytaleHelpTopicChoice::Overview),
    ))
    .await?;

    Ok(())
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Show the public Hytale server address and password")
)]
async fn join(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Hytale).await? {
        return Ok(());
    }

    let Some(guild_id) = ctx.guild_id() else {
        ctx.send(
            poise::CreateReply::default()
                .content("Hytale join info only works inside the Discord server.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    ctx.defer().await?;
    let public_ip = public_ip_for_join(ctx, guild_id.get()).await?;
    let password = ctx.data().settings.hytale_password(guild_id.get()).await;

    ctx.say(format_hytale_join_message(
        &public_ip,
        DEFAULT_HYTALE_PORT,
        &password,
    ))
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Check the Hytale service status")
)]
async fn status(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::Status).await
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Show recent Hytale service logs")
)]
async fn logs(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::Logs).await
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Start the Hytale service")
)]
async fn start(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::Start).await
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Stop the Hytale service")
)]
async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::Stop).await
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Restart the Hytale service")
)]
async fn restart(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::Restart).await
}

#[poise::command(
    slash_command,
    rename = "check-update",
    description_localized("en-US", "Check whether a Hytale server update is available")
)]
async fn check_update(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::CheckUpdate).await
}

#[poise::command(
    slash_command,
    description_localized("en-US", "Update the Hytale server and restart it")
)]
async fn update(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::Update).await
}

#[poise::command(slash_command, subcommands("set_channel", "set_password"))]
async fn set(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "channel",
    description_localized("en-US", "Set the only channel where Hytale commands work")
)]
async fn set_channel(
    ctx: Context<'_>,
    #[description = "Channel where Hytale commands should work"] channel: serenity::GuildChannel,
) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Hytale).await? {
        return Ok(());
    }
    let Some(_config) = hytale_config_for(ctx).await? else {
        return Ok(());
    };
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    ctx.data()
        .settings
        .set_channel(guild_id.get(), ChannelFamily::Hytale, channel.id.get())
        .await?;
    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "Hytale commands now only work in <#{}>.",
                channel.id.get()
            ))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "password",
    description_localized("en-US", "Set and enable the Hytale server password")
)]
async fn set_password(
    ctx: Context<'_>,
    #[description = "New Hytale server password"] password: String,
) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Hytale).await? {
        return Ok(());
    }
    let Some(config) = hytale_config_for(ctx).await? else {
        return Ok(());
    };
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let password = password.trim().to_owned();
    if password.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .content("Password cannot be empty.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;
    let password_settings = HytalePasswordSettings {
        password_enabled: true,
        last_password: Some(password.clone()),
    };
    write_hytale_password_config(&config.hytale_dir, &password_settings).await?;
    ctx.data()
        .settings
        .set_hytale_password(guild_id.get(), password, true)
        .await?;
    verify_hytale_password_settings_persisted(ctx, guild_id.get(), &password_settings).await?;
    send_ephemeral(
        ctx,
        "Hytale password updated and enabled. Restarting the server...",
    )
    .await?;
    restart_after_password_change(ctx, &config).await?;
    Ok(())
}

#[poise::command(slash_command, subcommands("toggle_password"))]
async fn toggle(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "password",
    description_localized("en-US", "Toggle Hytale server password protection")
)]
async fn toggle_password(ctx: Context<'_>) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Hytale).await? {
        return Ok(());
    }
    let Some(config) = hytale_config_for(ctx).await? else {
        return Ok(());
    };
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };

    let current = ctx.data().settings.hytale_password(guild_id.get()).await;
    let enabled = !current.password_enabled;
    if enabled && current.last_password.as_deref().unwrap_or("").is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .content("No Hytale password is saved yet. Use `/grate hytale set password` first.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;
    let password_settings = HytalePasswordSettings {
        password_enabled: enabled,
        last_password: current.last_password.clone(),
    };
    write_hytale_password_config(&config.hytale_dir, &password_settings).await?;
    ctx.data()
        .settings
        .set_hytale_password_enabled(guild_id.get(), enabled)
        .await?;
    verify_hytale_password_settings_persisted(ctx, guild_id.get(), &password_settings).await?;
    let state = if enabled { "enabled" } else { "disabled" };
    send_ephemeral(
        ctx,
        format!("Hytale password protection {state}. Restarting the server..."),
    )
    .await?;
    restart_after_password_change(ctx, &config).await?;
    Ok(())
}

async fn verify_hytale_password_settings_persisted(
    ctx: Context<'_>,
    guild_id: u64,
    expected: &HytalePasswordSettings,
) -> Result<(), Error> {
    let reloaded = crate::settings::SettingsStore::load(ctx.data().settings.path().to_path_buf())
        .await
        .context("could not reload bot settings after saving Hytale password state")?;
    let actual = reloaded.hytale_password(guild_id).await;
    if &actual != expected {
        anyhow::bail!(
            "saved Hytale password state did not match after reloading bot settings; server restart skipped"
        );
    }
    Ok(())
}

async fn run_hytale_command(ctx: Context<'_>, action: HytaleScriptAction) -> Result<(), Error> {
    if !ensure_command_channel(ctx, ChannelFamily::Hytale).await? {
        return Ok(());
    }

    let Some(config) = hytale_config_for(ctx).await? else {
        return Ok(());
    };

    ctx.defer_ephemeral().await?;
    send_ephemeral(ctx, format!("Starting Hytale {}...", action.arg())).await?;

    let output = match run_script_with_progress(ctx, &config, action).await {
        Ok(output) => output,
        Err(error) => {
            send_ephemeral(
                ctx,
                format!(
                    "Hytale {} failed: {}",
                    action.arg(),
                    truncate_inline(&format!("{error:#}"), MAX_RESPONSE_CHARS)
                ),
            )
            .await?;
            return Ok(());
        }
    };
    send_ephemeral(ctx, final_response(action, &output)).await?;

    Ok(())
}

async fn restart_after_password_change(
    ctx: Context<'_>,
    config: &HytaleConfig,
) -> Result<(), Error> {
    let output = match run_script_with_progress(ctx, config, HytaleScriptAction::Restart).await {
        Ok(output) => output,
        Err(error) => {
            send_ephemeral(
                ctx,
                format!(
                    "Hytale restart failed after password change: {}",
                    truncate_inline(&format!("{error:#}"), MAX_RESPONSE_CHARS)
                ),
            )
            .await?;
            return Ok(());
        }
    };

    send_ephemeral(ctx, final_response(HytaleScriptAction::Restart, &output)).await?;
    Ok(())
}

async fn hytale_config_for(ctx: Context<'_>) -> Result<Option<HytaleConfig>, Error> {
    if ctx.guild_id().is_none() {
        ctx.send(
            poise::CreateReply::default()
                .content("Hytale server controls only work inside the Discord server.")
                .ephemeral(true),
        )
        .await?;
        return Ok(None);
    }

    let config = match HytaleConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            ctx.send(
                poise::CreateReply::default()
                    .content(error.setup_message())
                    .ephemeral(true),
            )
            .await?;
            return Ok(None);
        }
    };

    let Some(member) = ctx.author_member().await else {
        ctx.send(
            poise::CreateReply::default()
                .content("I could not verify your server roles, so I cannot run Hytale controls.")
                .ephemeral(true),
        )
        .await?;
        return Ok(None);
    };

    if !member_has_role(&member, config.manager_role_id) {
        ctx.send(
            poise::CreateReply::default()
                .content("You do not have Hytale server manager permission.")
                .ephemeral(true),
        )
        .await?;
        return Ok(None);
    }

    Ok(Some(config))
}

fn member_has_role(member: &serenity::Member, role_id: RoleId) -> bool {
    member.roles.contains(&role_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HytaleScriptAction {
    Status,
    Logs,
    Start,
    Stop,
    Restart,
    CheckUpdate,
    Update,
}

impl HytaleScriptAction {
    #[cfg(test)]
    const ALL: [Self; 7] = [
        Self::Status,
        Self::Logs,
        Self::Start,
        Self::Stop,
        Self::Restart,
        Self::CheckUpdate,
        Self::Update,
    ];

    fn arg(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Logs => "logs",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::CheckUpdate => "check-update",
            Self::Update => "update",
        }
    }

    fn timeout(self, config: &HytaleConfig) -> Duration {
        match self {
            Self::CheckUpdate | Self::Update => config.download_timeout,
            Self::Status | Self::Logs | Self::Start | Self::Stop | Self::Restart => {
                config.command_timeout
            }
        }
    }

    fn shows_human_output(self) -> bool {
        matches!(self, Self::Status | Self::Logs | Self::CheckUpdate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSpec {
    program: PathBuf,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    timeout: Duration,
}

impl CommandSpec {
    fn script(config: &HytaleConfig, action: HytaleScriptAction) -> Self {
        let mut envs = vec![("SERVICE_NAME".to_owned(), config.service_name.clone())];
        if matches!(
            action,
            HytaleScriptAction::CheckUpdate | HytaleScriptAction::Update
        ) {
            envs.push((
                "DOWNLOAD_TIMEOUT_SECONDS".to_owned(),
                config.download_timeout.as_secs().to_string(),
            ));
        }

        Self {
            program: config.manage_script.clone(),
            args: vec![action.arg().to_owned()],
            envs,
            timeout: action.timeout(config),
        }
    }
}

#[derive(Debug, Clone)]
struct ScriptOutput {
    success: bool,
    human_output: String,
    latest_progress: Option<HytaleProgress>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct HytaleProgress {
    timestamp: String,
    source: String,
    stage: String,
    status: String,
    message: String,
}

async fn run_script_with_progress(
    ctx: Context<'_>,
    config: &HytaleConfig,
    action: HytaleScriptAction,
) -> Result<ScriptOutput, Error> {
    let spec = CommandSpec::script(config, action);
    timeout(spec.timeout, run_script_inner(ctx, spec, action))
        .await
        .with_context(|| {
            format!(
                "Hytale {} timed out after {}s",
                action.arg(),
                action.timeout(config).as_secs()
            )
        })?
}

async fn run_script_inner(
    ctx: Context<'_>,
    spec: CommandSpec,
    action: HytaleScriptAction,
) -> Result<ScriptOutput, Error> {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .envs(spec.envs)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .with_context(|| format!("could not start {}", spec.program.display()))?;

    let stdout = child
        .stdout
        .take()
        .context("could not capture script stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("could not capture script stderr")?;
    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stderr_lines = BufReader::new(stderr).lines();
    let mut wait_future = Box::pin(child.wait());

    let mut stdout_done = false;
    let mut stderr_done = false;
    let mut exit_status: Option<ExitStatus> = None;
    let mut human_lines = Vec::new();
    let mut latest_progress = None;
    let mut latest_progress_key = None;
    let mut latest_auth_url = None;

    while !(stdout_done && stderr_done && exit_status.is_some()) {
        tokio::select! {
            line = stdout_lines.next_line(), if !stdout_done => {
                match line? {
                    Some(line) => {
                        handle_script_line(
                            ctx,
                            action,
                            &line,
                            &mut human_lines,
                            &mut latest_progress,
                            &mut latest_progress_key,
                            &mut latest_auth_url,
                        ).await?;
                    }
                    None => stdout_done = true,
                }
            }
            line = stderr_lines.next_line(), if !stderr_done => {
                match line? {
                    Some(line) => {
                        handle_script_line(
                            ctx,
                            action,
                            &line,
                            &mut human_lines,
                            &mut latest_progress,
                            &mut latest_progress_key,
                            &mut latest_auth_url,
                        ).await?;
                    }
                    None => stderr_done = true,
                }
            }
            status = &mut wait_future, if exit_status.is_none() => {
                exit_status = Some(status?);
            }
        }
    }

    Ok(ScriptOutput {
        success: exit_status.is_some_and(|status| status.success()),
        human_output: joined_output(&human_lines),
        latest_progress,
    })
}

async fn handle_script_line(
    ctx: Context<'_>,
    action: HytaleScriptAction,
    line: &str,
    human_lines: &mut Vec<String>,
    latest_progress: &mut Option<HytaleProgress>,
    latest_progress_key: &mut Option<String>,
    latest_auth_url: &mut Option<String>,
) -> Result<(), Error> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    if let Some(progress) = parse_progress_line(trimmed) {
        let key = progress_key(&progress);
        if latest_progress_key.as_deref() != Some(key.as_str()) {
            send_ephemeral_best_effort(ctx, format_progress_message(action, &progress)).await;
            *latest_progress_key = Some(key);
        }
        *latest_progress = Some(progress);
    } else {
        if matches!(
            action,
            HytaleScriptAction::CheckUpdate | HytaleScriptAction::Update
        ) {
            maybe_send_auth_url(ctx, trimmed, latest_auth_url).await;
        }
        human_lines.push(line.to_owned());
    }

    Ok(())
}

async fn maybe_send_auth_url(ctx: Context<'_>, line: &str, latest_auth_url: &mut Option<String>) {
    let Some(url) = extract_first_url(line) else {
        return;
    };

    if latest_auth_url.as_deref() == Some(url.as_str()) {
        return;
    }

    send_ephemeral_best_effort(ctx, format!("Hytale update auth link: {url}")).await;
    *latest_auth_url = Some(url);
}

fn parse_progress_line(line: &str) -> Option<HytaleProgress> {
    serde_json::from_str::<HytaleProgress>(line).ok()
}

fn extract_first_url(line: &str) -> Option<String> {
    let start = line.find("https://").or_else(|| line.find("http://"))?;
    let url = line[start..]
        .split(|character: char| character.is_whitespace() || matches!(character, '<' | '>' | ')'))
        .next()?
        .trim_end_matches(['.', ',', ';', ':'])
        .to_owned();

    (!url.is_empty()).then_some(url)
}

fn progress_key(progress: &HytaleProgress) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        progress.source, progress.stage, progress.status, progress.message
    )
}

fn format_progress_message(action: HytaleScriptAction, progress: &HytaleProgress) -> String {
    let progress = display_progress(progress);
    format!(
        "Hytale {} {}: {}",
        action.arg(),
        progress.status,
        truncate_inline(&progress.message, 1_500)
    )
}

fn display_progress(progress: &HytaleProgress) -> HytaleProgress {
    const PREFIX: &str = "updater progress: ";

    progress
        .message
        .strip_prefix(PREFIX)
        .and_then(parse_progress_line)
        .unwrap_or_else(|| progress.clone())
}

fn final_response(action: HytaleScriptAction, output: &ScriptOutput) -> String {
    if action.shows_human_output() {
        return final_output_response(action, output);
    }

    let label = action.arg();
    let progress = output.latest_progress.as_ref().map(display_progress);
    let state = progress
        .as_ref()
        .map(|progress| progress.status.as_str())
        .unwrap_or(if output.success {
            "completed"
        } else {
            "failed"
        });
    let message = progress
        .as_ref()
        .map(|progress| progress.message.as_str())
        .unwrap_or(if output.success {
            "script completed"
        } else {
            "script failed"
        });

    if output.success {
        format!("Hytale {label} {state}: {message}")
    } else {
        let hint = failure_hint(&output.human_output);
        format!(
            "Hytale {label} {state}: {message}\n{}{}",
            code_block(&truncate_text(&output.human_output, MAX_RESPONSE_CHARS)),
            hint.map(|hint| format!("\n\n{hint}")).unwrap_or_default()
        )
    }
}

fn final_output_response(action: HytaleScriptAction, output: &ScriptOutput) -> String {
    let title = match action {
        HytaleScriptAction::Status => "Hytale status",
        HytaleScriptAction::Logs => "Recent Hytale server logs",
        HytaleScriptAction::CheckUpdate => "Hytale update check",
        _ => "Hytale output",
    };
    let body = if output.success {
        output.human_output.clone()
    } else {
        let hint = failure_hint(&output.human_output)
            .map(|hint| format!("\n\n{hint}"))
            .unwrap_or_default();
        format!(
            "The Hytale {} command failed.\n\n{}{}",
            action.arg(),
            output.human_output,
            hint
        )
    };

    format!(
        "{title}:\n{}",
        code_block(&truncate_text(&body, MAX_RESPONSE_CHARS))
    )
}

async fn public_ip_for_join(ctx: Context<'_>, guild_id: u64) -> Result<String, Error> {
    let cached_ip = ctx.data().settings.hytale_cached_public_ip(guild_id).await;
    match fetch_public_ip().await {
        Ok(public_ip) => {
            ctx.data()
                .settings
                .set_hytale_cached_public_ip(guild_id, public_ip.clone())
                .await?;
            Ok(public_ip)
        }
        Err(error) => cached_ip.ok_or(error),
    }
}

async fn fetch_public_ip() -> Result<String, Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .context("could not build public IP lookup client")?;
    let ip_text = client
        .get("https://api.getpublicip.com/ip")
        .send()
        .await
        .context("could not look up the public Hytale server IP")?
        .error_for_status()
        .context("public Hytale server IP lookup failed")?
        .text()
        .await
        .context("could not read public Hytale server IP response")?;
    let ip = ip_text.trim();
    ip.parse::<IpAddr>()
        .with_context(|| format!("public IP lookup returned an invalid IP address: {ip}"))?;
    Ok(ip.to_owned())
}

fn format_hytale_join_message(
    public_ip: &str,
    port: u16,
    password: &HytalePasswordSettings,
) -> String {
    let mut message = format!("Hytale server\nAddress: `{public_ip}:{port}`");
    if password.password_enabled {
        if let Some(password) = password
            .last_password
            .as_deref()
            .filter(|password| !password.is_empty())
        {
            message.push_str(&format!("\nPassword: `{}`", password.replace('`', "'")));
        }
    }
    message
}

async fn read_hytale_server_config(hytale_dir: &PathBuf) -> Result<Value, Error> {
    let path = hytale_config_path(hytale_dir);
    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => serde_json::from_str(&contents)
            .with_context(|| format!("could not parse {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

async fn write_hytale_password_config(
    hytale_dir: &PathBuf,
    password: &HytalePasswordSettings,
) -> Result<(), Error> {
    let mut config = read_hytale_server_config(hytale_dir).await?;
    let object = ensure_json_object(&mut config);
    object.insert(
        "GrateBot".to_owned(),
        serde_json::json!({
            "password_enabled": password.password_enabled,
            "password": password.last_password.clone(),
        }),
    );

    let path = hytale_config_path(hytale_dir);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("could not create {}", parent.display()))?;
    }

    let contents = serde_json::to_vec_pretty(&config)?;
    let temp_path = path.with_file_name("config.json.tmp");
    tokio::fs::write(&temp_path, contents)
        .await
        .with_context(|| format!("could not write {}", temp_path.display()))?;
    tokio::fs::rename(&temp_path, &path)
        .await
        .with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

fn ensure_json_object(value: &mut Value) -> &mut Map<String, Value> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    value
        .as_object_mut()
        .expect("value was just made an object")
}

fn hytale_config_path(hytale_dir: &PathBuf) -> PathBuf {
    hytale_dir.join("Server/config.json")
}

fn failure_hint(output: &str) -> Option<&'static str> {
    let normalized = output.to_ascii_lowercase();
    if normalized.contains("sudo: a password is required")
        || normalized.contains("sudo: no tty present")
        || normalized.contains("sudo: a terminal is required")
    {
        return Some(
            "Hint: the bot host user cannot run the Hytale management sudo commands without a password. Configure passwordless sudo for that user, then verify it on the host with `sudo -n systemctl status hytale-server.service --no-pager` or the configured service name.",
        );
    }

    None
}

fn joined_output(lines: &[String]) -> String {
    if lines.is_empty() {
        "(no output)".to_owned()
    } else {
        lines.join("\n")
    }
}

fn code_block(value: &str) -> String {
    format!("```text\n{}\n```", value.replace("```", "'''"))
}

async fn send_ephemeral(ctx: Context<'_>, content: impl Into<String>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default()
            .content(content.into())
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

async fn send_ephemeral_best_effort(ctx: Context<'_>, content: impl Into<String>) {
    if let Err(error) = send_ephemeral(ctx, content).await {
        eprintln!("could not send Hytale progress update: {error:#}");
    }
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();

    if chars.next().is_some() {
        format!("{truncated}\n... output trimmed ...")
    } else {
        truncated
    }
}

fn truncate_inline(value: &str, max_chars: usize) -> String {
    truncate_text(value, max_chars).replace('\n', " ")
}

fn hytale_help_text(topic: HytaleHelpTopicChoice) -> &'static str {
    match topic {
        HytaleHelpTopicChoice::Overview => {
            "Hytale help: these commands let trusted server helpers check, manage, and update the hosted Hytale server. Everyone can use `/grate hytale join` for public join info.\n\nUse the `topic` option on `/grate hytale help` for focused help: `commands`, `settings`, `permissions`, `operations flow`, or `troubleshooting`.\n\nDefault behavior: the bot calls `~/hytale/hytale-manage.sh`, manages `hytale-server.service`, waits up to 15 seconds for regular commands, and waits up to 1800 seconds for update checks and updates unless the bot owner configured different environment variables. Management commands require the Hytale manager role."
        }
        HytaleHelpTopicChoice::Commands => {
            "Hytale commands:\n`/grate hytale help`: explain commands, settings, permissions, and troubleshooting.\n`/grate hytale join`: print public server address and password when enabled.\n`/grate hytale status`: checks the service status using the management script.\n`/grate hytale logs`: shows recent service logs using the management script.\n`/grate hytale start`: starts the server.\n`/grate hytale stop`: stops the server.\n`/grate hytale restart`: restarts the server.\n`/grate hytale check-update`: checks whether a server update is available without applying it.\n`/grate hytale update`: stops the server if needed, updates it, and starts it again.\n`/grate hytale set channel`: set the only channel where Hytale commands work.\n`/grate hytale set password`: set and enable the server password.\n`/grate hytale toggle password`: turn password protection on or off.\n\nManager commands are ephemeral and require the configured manager role."
        }
        HytaleHelpTopicChoice::Settings => {
            "Hytale settings for the bot owner:\n`HYTALE_MANAGER_ROLE_ID`: required Discord role ID allowed to use Hytale controls.\n`HYTALE_MANAGE_SCRIPT`: optional path to `hytale-manage.sh`. Defaults to `~/hytale/hytale-manage.sh`.\n`HYTALE_DIR`: optional Hytale install directory containing `Server/config.json`. Defaults to `~/hytale`.\n`HYTALE_SERVICE_NAME`: optional systemd service name passed to the script as `SERVICE_NAME`. Defaults to `hytale-server.service`.\n`HYTALE_COMMAND_TIMEOUT_SECONDS`: optional timeout for status, logs, start, stop, and restart. Defaults to 15 seconds, with a minimum of 1 second.\n`HYTALE_DOWNLOAD_TIMEOUT_SECONDS`: optional timeout for `/grate hytale check-update` and `/grate hytale update`, also passed to the script as `DOWNLOAD_TIMEOUT_SECONDS`. Defaults to 1800 seconds, with a minimum of 1 second.\n\nIf the required role ID is missing or invalid, management commands explain the setup problem instead of running."
        }
        HytaleHelpTopicChoice::Permissions => {
            "Hytale permissions:\nOnly members with the configured Hytale manager role can run `status`, `logs`, `start`, `stop`, `restart`, `check-update`, `update`, `set channel`, `set password`, or `toggle password`.\n\nThe bot only calls the configured `hytale-manage.sh` script with one of those fixed service actions. The script handles systemd, logs, sudo, and update work on the host.\n\nThe help and join commands are available without the manager role so people can discover how the controls work and connect to the server."
        }
        HytaleHelpTopicChoice::OperationsFlow => {
            "Typical Hytale operations flow:\n1. Run `/grate hytale status` to see whether the service is active or failed.\n2. If players report issues, run `/grate hytale logs` and scan recent output.\n3. Use `/grate hytale start` only when the server is stopped.\n4. Use `/grate hytale restart` when the server is wedged and logs/status suggest a restart is appropriate.\n5. Use `/grate hytale stop` when intentionally taking the server offline.\n6. Use `/grate hytale check-update` to see whether a new server build is available.\n7. Use `/grate hytale update` when applying a new server build.\n8. Re-check `/grate hytale status` after start, stop, restart, or update."
        }
        HytaleHelpTopicChoice::Troubleshooting => {
            "Hytale troubleshooting:\nIf commands say controls are not set up, the bot owner needs to set `HYTALE_MANAGER_ROLE_ID`.\nIf you lack permission, ask for the configured Hytale manager role.\nIf a command fails to start, check `HYTALE_MANAGE_SCRIPT` and make sure the script exists and is executable by the bot user.\nIf a script action fails, check host sudoers, systemd permissions, journal access, and the script output shown in Discord.\nIf update waits for auth, complete the Hytale downloader authorization flow on the host.\nIf output is trimmed, use host access for deeper investigation; Discord replies intentionally cap long command output."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<T>(vars: &[(&str, Option<&str>)], test: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = vars
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();

        for (key, value) in vars {
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }

        let result = test();

        for (key, value) in previous {
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }

        result
    }

    fn test_config() -> HytaleConfig {
        HytaleConfig {
            manager_role_id: RoleId::new(12345),
            service_name: "hytale-server.service".to_owned(),
            manage_script: PathBuf::from("/opt/hytale/hytale-manage.sh"),
            hytale_dir: PathBuf::from("/opt/hytale"),
            command_timeout: Duration::from_secs(15),
            download_timeout: Duration::from_secs(1_800),
        }
    }

    #[test]
    fn config_uses_defaults_and_required_role() {
        with_env(
            &[
                ("HYTALE_MANAGER_ROLE_ID", Some("12345")),
                ("HYTALE_SERVICE_NAME", None),
                ("HYTALE_MANAGE_SCRIPT", None),
                ("HYTALE_DIR", None),
                ("HYTALE_COMMAND_TIMEOUT_SECONDS", None),
                ("HYTALE_DOWNLOAD_TIMEOUT_SECONDS", None),
                ("HOME", Some("/home/bot")),
            ],
            || {
                let config = HytaleConfig::from_env().unwrap();

                assert_eq!(config.manager_role_id, RoleId::new(12345));
                assert_eq!(config.service_name, DEFAULT_SERVICE_NAME);
                assert_eq!(
                    config.manage_script,
                    PathBuf::from("/home/bot/hytale/hytale-manage.sh")
                );
                assert_eq!(config.hytale_dir, PathBuf::from("/home/bot/hytale"));
                assert_eq!(
                    config.command_timeout,
                    Duration::from_secs(DEFAULT_COMMAND_TIMEOUT_SECONDS)
                );
                assert_eq!(
                    config.download_timeout,
                    Duration::from_secs(DEFAULT_DOWNLOAD_TIMEOUT_SECONDS)
                );
            },
        );
    }

    #[test]
    fn config_fails_closed_without_role() {
        with_env(&[("HYTALE_MANAGER_ROLE_ID", None)], || {
            assert_eq!(HytaleConfig::from_env(), Err(ConfigError::MissingRole));
        });
    }

    #[test]
    fn config_rejects_invalid_role() {
        with_env(&[("HYTALE_MANAGER_ROLE_ID", Some("not-a-role"))], || {
            assert_eq!(
                HytaleConfig::from_env(),
                Err(ConfigError::InvalidRole("not-a-role".to_owned()))
            );
        });
    }

    #[test]
    fn config_reads_overrides() {
        with_env(
            &[
                ("HYTALE_MANAGER_ROLE_ID", Some("12345")),
                ("HYTALE_SERVICE_NAME", Some("custom-hytale.service")),
                ("HYTALE_MANAGE_SCRIPT", Some("/srv/hytale/manage")),
                ("HYTALE_DIR", Some("/srv/hytale/server")),
                ("HYTALE_COMMAND_TIMEOUT_SECONDS", Some("22")),
                ("HYTALE_DOWNLOAD_TIMEOUT_SECONDS", Some("2400")),
            ],
            || {
                let config = HytaleConfig::from_env().unwrap();

                assert_eq!(config.service_name, "custom-hytale.service");
                assert_eq!(config.manage_script, PathBuf::from("/srv/hytale/manage"));
                assert_eq!(config.hytale_dir, PathBuf::from("/srv/hytale/server"));
                assert_eq!(config.command_timeout, Duration::from_secs(22));
                assert_eq!(config.download_timeout, Duration::from_secs(2_400));
            },
        );
    }

    #[test]
    fn command_specs_are_allowlisted_script_actions() {
        let config = test_config();
        let args = HytaleScriptAction::ALL
            .into_iter()
            .map(|action| CommandSpec::script(&config, action).args)
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            vec![
                vec!["status".to_owned()],
                vec!["logs".to_owned()],
                vec!["start".to_owned()],
                vec!["stop".to_owned()],
                vec!["restart".to_owned()],
                vec!["check-update".to_owned()],
                vec!["update".to_owned()],
            ]
        );
    }

    #[test]
    fn script_spec_sets_program_env_and_timeout() {
        let config = test_config();
        let spec = CommandSpec::script(&config, HytaleScriptAction::Restart);

        assert_eq!(spec.program, PathBuf::from("/opt/hytale/hytale-manage.sh"));
        assert_eq!(spec.args, vec!["restart".to_owned()]);
        assert_eq!(
            spec.envs,
            vec![(
                "SERVICE_NAME".to_owned(),
                "hytale-server.service".to_owned()
            )]
        );
        assert_eq!(spec.timeout, Duration::from_secs(15));
    }

    #[test]
    fn update_spec_uses_download_timeout_and_env() {
        let config = test_config();
        let spec = CommandSpec::script(&config, HytaleScriptAction::Update);

        assert_eq!(spec.args, vec!["update".to_owned()]);
        assert_eq!(
            spec.envs,
            vec![
                (
                    "SERVICE_NAME".to_owned(),
                    "hytale-server.service".to_owned()
                ),
                ("DOWNLOAD_TIMEOUT_SECONDS".to_owned(), "1800".to_owned()),
            ]
        );
        assert_eq!(spec.timeout, Duration::from_secs(1_800));
    }

    #[test]
    fn check_update_spec_uses_download_timeout_and_env() {
        let config = test_config();
        let spec = CommandSpec::script(&config, HytaleScriptAction::CheckUpdate);

        assert_eq!(spec.args, vec!["check-update".to_owned()]);
        assert_eq!(
            spec.envs,
            vec![
                (
                    "SERVICE_NAME".to_owned(),
                    "hytale-server.service".to_owned()
                ),
                ("DOWNLOAD_TIMEOUT_SECONDS".to_owned(), "1800".to_owned()),
            ]
        );
        assert_eq!(spec.timeout, Duration::from_secs(1_800));
    }

    #[test]
    fn parses_json_progress() {
        let progress = parse_progress_line(
            r#"{"timestamp":"2026-04-30T13:10:02Z","source":"hytale-manage","stage":"update","status":"running","message":"running updater"}"#,
        )
        .unwrap();

        assert_eq!(progress.stage, "update");
        assert_eq!(progress.status, "running");
        assert_eq!(progress.message, "running updater");
    }

    #[test]
    fn ignores_malformed_progress() {
        assert!(parse_progress_line("[hytale-manage] starting").is_none());
    }

    #[test]
    fn extracts_auth_url_from_output_line() {
        assert_eq!(
            extract_first_url("Please visit https://example.com/device?code=abc to authenticate"),
            Some("https://example.com/device?code=abc".to_owned())
        );
        assert_eq!(
            extract_first_url("<https://example.com/device>,"),
            Some("https://example.com/device".to_owned())
        );
        assert_eq!(extract_first_url("waiting for auth"), None);
    }

    #[test]
    fn formats_nested_updater_progress() {
        let progress = parse_progress_line(
            r#"{"timestamp":"2026-04-30T13:10:02Z","source":"hytale-manage","stage":"update","status":"running","message":"updater progress: {\"timestamp\":\"2026-04-30T13:11:02Z\",\"source\":\"hytale-update\",\"stage\":\"auth\",\"status\":\"waiting\",\"message\":\"waiting for Hytale device authorization to complete\"}"}"#,
        )
        .unwrap();

        let message = format_progress_message(HytaleScriptAction::Update, &progress);

        assert_eq!(
            message,
            "Hytale update waiting: waiting for Hytale device authorization to complete"
        );
    }

    #[test]
    fn formats_success_response() {
        let output = ScriptOutput {
            success: true,
            human_output: "(no output)".to_owned(),
            latest_progress: Some(parse_progress_line(
                r#"{"timestamp":"2026-04-30T13:15:40Z","source":"hytale-manage","stage":"restart","status":"completed","message":"hytale-server is active after restart"}"#,
            ).unwrap()),
        };

        assert_eq!(
            final_response(HytaleScriptAction::Restart, &output),
            "Hytale restart completed: hytale-server is active after restart"
        );
    }

    #[test]
    fn formats_failure_response_with_output() {
        let output = ScriptOutput {
            success: false,
            human_output: "systemctl refused".to_owned(),
            latest_progress: Some(parse_progress_line(
                r#"{"timestamp":"2026-04-30T13:15:40Z","source":"hytale-manage","stage":"start","status":"failed","message":"command failed with exit code 1"}"#,
            ).unwrap()),
        };
        let response = final_response(HytaleScriptAction::Start, &output);

        assert!(response.contains("Hytale start failed: command failed with exit code 1"));
        assert!(response.contains("systemctl refused"));
    }

    #[test]
    fn formats_sudo_password_failure_with_hint() {
        let output = ScriptOutput {
            success: false,
            human_output: "sudo: a password is required\nsudo: a password is required".to_owned(),
            latest_progress: Some(parse_progress_line(
                r#"{"timestamp":"2026-04-30T13:15:40Z","source":"hytale-manage","stage":"update","status":"failed","message":"command failed with exit code 1"}"#,
            ).unwrap()),
        };
        let response = final_response(HytaleScriptAction::Update, &output);

        assert!(response.contains("Hytale update failed: command failed with exit code 1"));
        assert!(response.contains("passwordless sudo"));
        assert!(response.contains("sudo -n systemctl status hytale-server.service --no-pager"));
    }

    #[test]
    fn formats_status_output_and_trims() {
        let output = ScriptOutput {
            success: true,
            human_output: "abcdef".to_owned(),
            latest_progress: None,
        };

        let response = final_output_response(HytaleScriptAction::Status, &output);

        assert!(response.contains("Hytale status"));
        assert!(response.contains("abcdef"));
        assert_eq!(truncate_text("abcdef", 3), "abc\n... output trimmed ...");
    }

    #[test]
    fn formats_check_update_output() {
        let output = ScriptOutput {
            success: true,
            human_output: "Update available: 2026.5.13".to_owned(),
            latest_progress: None,
        };

        let response = final_output_response(HytaleScriptAction::CheckUpdate, &output);

        assert!(response.contains("Hytale update check"));
        assert!(response.contains("Update available"));
    }

    #[test]
    fn formats_hytale_join_with_password_only_when_enabled() {
        let disabled = HytalePasswordSettings {
            password_enabled: false,
            last_password: Some("secret".to_owned()),
        };
        assert_eq!(
            format_hytale_join_message("203.0.113.10", 5520, &disabled),
            "Hytale server\nAddress: `203.0.113.10:5520`"
        );

        let enabled = HytalePasswordSettings {
            password_enabled: true,
            last_password: Some("sec`ret".to_owned()),
        };
        assert_eq!(
            format_hytale_join_message("203.0.113.10", 5520, &enabled),
            "Hytale server\nAddress: `203.0.113.10:5520`\nPassword: `sec'ret`"
        );
    }

    #[tokio::test]
    async fn writes_gratebot_password_config_and_preserves_unknown_fields() {
        let root =
            std::env::temp_dir().join(format!("grate-bot-hytale-config-{}", std::process::id()));
        let server_dir = root.join("Server");
        let config_path = server_dir.join("config.json");
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&server_dir).await.unwrap();
        tokio::fs::write(
            &config_path,
            r#"{"bind_port":5520,"nested":{"keep":true},"GrateBot":{"password_enabled":false,"password":"old"}}"#,
        )
        .await
        .unwrap();

        write_hytale_password_config(
            &root,
            &HytalePasswordSettings {
                password_enabled: true,
                last_password: Some("new-pass".to_owned()),
            },
        )
        .await
        .unwrap();

        let config = read_hytale_server_config(&root).await.unwrap();
        assert_eq!(config["bind_port"], serde_json::json!(5520));
        assert_eq!(config["nested"]["keep"], serde_json::json!(true));
        assert_eq!(
            config["GrateBot"]["password_enabled"],
            serde_json::json!(true)
        );
        assert_eq!(
            config["GrateBot"]["password"],
            serde_json::json!("new-pass")
        );

        let _ = tokio::fs::remove_dir_all(&root).await;
    }
}

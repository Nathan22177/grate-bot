use crate::bot::Context;
use anyhow::Context as AnyhowContext;
use poise::serenity_prelude as serenity;
use serde::Deserialize;
use serenity::RoleId;
use std::{
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

#[poise::command(
    slash_command,
    subcommands("help", "status", "logs", "start", "stop", "restart", "update")
)]
pub async fn hytale(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
async fn help(
    ctx: Context<'_>,
    #[description = "What to explain; defaults to overview"] topic: Option<HytaleHelpTopicChoice>,
) -> Result<(), Error> {
    ctx.say(hytale_help_text(
        topic.unwrap_or(HytaleHelpTopicChoice::Overview),
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
    description_localized("en-US", "Update the Hytale server and restart it")
)]
async fn update(ctx: Context<'_>) -> Result<(), Error> {
    run_hytale_command(ctx, HytaleScriptAction::Update).await
}

async fn run_hytale_command(ctx: Context<'_>, action: HytaleScriptAction) -> Result<(), Error> {
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
    Update,
}

impl HytaleScriptAction {
    #[cfg(test)]
    const ALL: [Self; 6] = [
        Self::Status,
        Self::Logs,
        Self::Start,
        Self::Stop,
        Self::Restart,
        Self::Update,
    ];

    fn arg(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Logs => "logs",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Update => "update",
        }
    }

    fn timeout(self, config: &HytaleConfig) -> Duration {
        match self {
            Self::Update => config.download_timeout,
            Self::Status | Self::Logs | Self::Start | Self::Stop | Self::Restart => {
                config.command_timeout
            }
        }
    }

    fn shows_human_output(self) -> bool {
        matches!(self, Self::Status | Self::Logs)
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
        if action == HytaleScriptAction::Update {
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
        if action == HytaleScriptAction::Update {
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
        format!(
            "Hytale {label} {state}: {message}\n{}",
            code_block(&truncate_text(&output.human_output, MAX_RESPONSE_CHARS))
        )
    }
}

fn final_output_response(action: HytaleScriptAction, output: &ScriptOutput) -> String {
    let title = match action {
        HytaleScriptAction::Status => "Hytale status",
        HytaleScriptAction::Logs => "Recent Hytale server logs",
        _ => "Hytale output",
    };
    let body = if output.success {
        output.human_output.clone()
    } else {
        format!(
            "The Hytale {} command failed.\n\n{}",
            action.arg(),
            output.human_output
        )
    };

    format!(
        "{title}:\n{}",
        code_block(&truncate_text(&body, MAX_RESPONSE_CHARS))
    )
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
            "Hytale help: these commands let trusted server helpers check, manage, and update the hosted Hytale server.\n\nUse the `topic` option on `/grate hytale help` for focused help: `commands`, `settings`, `permissions`, `operations flow`, or `troubleshooting`.\n\nDefault behavior: the bot calls `~/hytale/hytale-manage.sh`, manages `hytale-server.service`, waits up to 15 seconds for regular commands, and waits up to 1800 seconds for updates unless the bot owner configured different environment variables. All management commands require the Hytale manager role."
        }
        HytaleHelpTopicChoice::Commands => {
            "Hytale commands:\n`/grate hytale help`: explain commands, settings, permissions, and troubleshooting.\n`/grate hytale status`: checks the service status using the management script.\n`/grate hytale logs`: shows recent service logs using the management script.\n`/grate hytale start`: starts the server.\n`/grate hytale stop`: stops the server.\n`/grate hytale restart`: restarts the server.\n`/grate hytale update`: stops the server if needed, updates it, and starts it again.\n\nAll operational commands are ephemeral and require the configured manager role."
        }
        HytaleHelpTopicChoice::Settings => {
            "Hytale settings for the bot owner:\n`HYTALE_MANAGER_ROLE_ID`: required Discord role ID allowed to use Hytale controls.\n`HYTALE_MANAGE_SCRIPT`: optional path to `hytale-manage.sh`. Defaults to `~/hytale/hytale-manage.sh`.\n`HYTALE_SERVICE_NAME`: optional systemd service name passed to the script as `SERVICE_NAME`. Defaults to `hytale-server.service`.\n`HYTALE_COMMAND_TIMEOUT_SECONDS`: optional timeout for status, logs, start, stop, and restart. Defaults to 15 seconds, with a minimum of 1 second.\n`HYTALE_DOWNLOAD_TIMEOUT_SECONDS`: optional timeout for `/grate hytale update`, also passed to the script as `DOWNLOAD_TIMEOUT_SECONDS`. Defaults to 1800 seconds, with a minimum of 1 second.\n\nIf the required role ID is missing or invalid, management commands explain the setup problem instead of running."
        }
        HytaleHelpTopicChoice::Permissions => {
            "Hytale permissions:\nOnly members with the configured Hytale manager role can run `status`, `logs`, `start`, `stop`, `restart`, or `update`.\n\nThe bot only calls the configured `hytale-manage.sh` script with one of those fixed actions. The script handles systemd, logs, sudo, and update work on the host.\n\nThe help command is available without the manager role so people can discover how the controls work."
        }
        HytaleHelpTopicChoice::OperationsFlow => {
            "Typical Hytale operations flow:\n1. Run `/grate hytale status` to see whether the service is active or failed.\n2. If players report issues, run `/grate hytale logs` and scan recent output.\n3. Use `/grate hytale start` only when the server is stopped.\n4. Use `/grate hytale restart` when the server is wedged and logs/status suggest a restart is appropriate.\n5. Use `/grate hytale stop` when intentionally taking the server offline.\n6. Use `/grate hytale update` when applying a new server build.\n7. Re-check `/grate hytale status` after start, stop, restart, or update."
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
                ("HYTALE_COMMAND_TIMEOUT_SECONDS", Some("22")),
                ("HYTALE_DOWNLOAD_TIMEOUT_SECONDS", Some("2400")),
            ],
            || {
                let config = HytaleConfig::from_env().unwrap();

                assert_eq!(config.service_name, "custom-hytale.service");
                assert_eq!(config.manage_script, PathBuf::from("/srv/hytale/manage"));
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
}

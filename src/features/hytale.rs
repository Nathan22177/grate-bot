use crate::bot::Context;
use anyhow::Context as AnyhowContext;
use poise::serenity_prelude as serenity;
use serenity::RoleId;
use std::{borrow::Cow, process::ExitStatus, time::Duration};
use tokio::{process::Command, time::timeout};

type Error = anyhow::Error;

const DEFAULT_SERVICE_NAME: &str = "hytale-server.service";
const DEFAULT_LOG_LINES: u16 = 40;
const MAX_LOG_LINES: u16 = 100;
const DEFAULT_TIMEOUT_SECONDS: u64 = 15;
const MAX_RESPONSE_CHARS: usize = 1_750;

#[derive(Debug, Clone, PartialEq, Eq)]
struct HytaleConfig {
    manager_role_id: RoleId,
    service_name: String,
    log_lines: u16,
    timeout: Duration,
}

impl HytaleConfig {
    fn from_env() -> Result<Self, ConfigError> {
        let manager_role_id = read_role_id("HYTALE_MANAGER_ROLE_ID")?;
        let service_name = std::env::var("HYTALE_SERVICE_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SERVICE_NAME.to_owned());
        let log_lines = read_u16("HYTALE_LOG_LINES", DEFAULT_LOG_LINES).min(MAX_LOG_LINES);
        let timeout_seconds = read_u64("HYTALE_COMMAND_TIMEOUT_SECONDS", DEFAULT_TIMEOUT_SECONDS);

        Ok(Self {
            manager_role_id,
            service_name,
            log_lines,
            timeout: Duration::from_secs(timeout_seconds.max(1)),
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

fn read_u16(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

fn read_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

#[poise::command(
    slash_command,
    subcommands("status", "logs", "start", "stop", "restart")
)]
pub async fn hytale(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let Some(config) = hytale_config_for(ctx).await? else {
        return Ok(());
    };

    ctx.defer_ephemeral().await?;
    let report = service_status(&config).await?;

    ctx.say(report).await?;
    Ok(())
}

#[poise::command(slash_command)]
async fn logs(ctx: Context<'_>) -> Result<(), Error> {
    let Some(config) = hytale_config_for(ctx).await? else {
        return Ok(());
    };

    ctx.defer_ephemeral().await?;
    let output = run_command(CommandSpec::logs(&config), config.timeout).await?;
    let body = if output.success {
        output.combined_output()
    } else {
        format!(
            "I could not read the Hytale logs.\n\n{}",
            output.combined_output()
        )
    };

    ctx.say(format!(
        "Recent Hytale server logs:\n{}",
        code_block(&truncate_text(&body, MAX_RESPONSE_CHARS))
    ))
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn start(ctx: Context<'_>) -> Result<(), Error> {
    run_service_action(ctx, ServiceAction::Start).await
}

#[poise::command(slash_command)]
async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    run_service_action(ctx, ServiceAction::Stop).await
}

#[poise::command(slash_command)]
async fn restart(ctx: Context<'_>) -> Result<(), Error> {
    run_service_action(ctx, ServiceAction::Restart).await
}

async fn run_service_action(ctx: Context<'_>, action: ServiceAction) -> Result<(), Error> {
    let Some(config) = hytale_config_for(ctx).await? else {
        return Ok(());
    };

    ctx.defer_ephemeral().await?;

    let output = run_command(CommandSpec::service_action(&config, action), config.timeout).await?;
    let content = if output.success {
        format!(
            "Hytale server {}. Use `/grate hytale status` if you want to check it.",
            action.success_phrase()
        )
    } else {
        format!(
            "I tried to {} the Hytale server, but the host refused or failed the command.\n{}",
            action.verb(),
            code_block(&truncate_text(
                &output.combined_output(),
                MAX_RESPONSE_CHARS
            ))
        )
    };

    ctx.say(content).await?;
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

fn member_has_role(member: &Cow<'_, serenity::Member>, role_id: RoleId) -> bool {
    member.roles.iter().any(|role| *role == role_id)
}

async fn service_status(config: &HytaleConfig) -> Result<String, Error> {
    let active = run_command(CommandSpec::is_active(config), config.timeout).await?;
    let enabled = run_command(CommandSpec::is_enabled(config), config.timeout).await?;
    let system_status = run_command(CommandSpec::system_status(config), config.timeout).await?;

    Ok(format_status_report(
        &config.service_name,
        active.stdout.trim(),
        enabled.stdout.trim(),
        &system_status.combined_output(),
    ))
}

fn format_status_report(
    service_name: &str,
    active_state: &str,
    enabled_state: &str,
    raw_status: &str,
) -> String {
    let friendly_state = match active_state {
        "active" => "The Hytale server is running.",
        "inactive" => "The Hytale server is stopped.",
        "activating" => "The Hytale server is starting up.",
        "deactivating" => "The Hytale server is shutting down.",
        "failed" => "The Hytale server tried to run but failed.",
        _ => "I could not clearly tell whether the Hytale server is running.",
    };

    let boot_state = match enabled_state {
        "enabled" => "It is set to start automatically when the host reboots.",
        "disabled" => "It is not set to start automatically when the host reboots.",
        _ => "I could not clearly tell whether it starts automatically on reboot.",
    };

    format!(
        "Hytale service: `{service_name}`\nState: `{}`\nBoot: `{}`\n\n{friendly_state} {boot_state}\n\nSystem details:\n{}",
        empty_unknown(active_state),
        empty_unknown(enabled_state),
        code_block(&truncate_text(raw_status, MAX_RESPONSE_CHARS))
    )
}

fn empty_unknown(value: &str) -> &str {
    if value.trim().is_empty() {
        "unknown"
    } else {
        value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceAction {
    Start,
    Stop,
    Restart,
}

impl ServiceAction {
    fn systemctl_arg(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    fn verb(self) -> &'static str {
        self.systemctl_arg()
    }

    fn success_phrase(self) -> &'static str {
        match self {
            Self::Start => "start command was accepted",
            Self::Stop => "stop command was accepted",
            Self::Restart => "restart command was accepted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandSpec {
    program: &'static str,
    args: Vec<String>,
}

impl CommandSpec {
    fn is_active(config: &HytaleConfig) -> Self {
        Self::systemctl(vec!["is-active".to_owned(), config.service_name.clone()])
    }

    fn is_enabled(config: &HytaleConfig) -> Self {
        Self::systemctl(vec!["is-enabled".to_owned(), config.service_name.clone()])
    }

    fn system_status(config: &HytaleConfig) -> Self {
        Self::systemctl(vec![
            "status".to_owned(),
            config.service_name.clone(),
            "--no-pager".to_owned(),
        ])
    }

    fn logs(config: &HytaleConfig) -> Self {
        Self {
            program: "journalctl",
            args: vec![
                "-u".to_owned(),
                config.service_name.clone(),
                "-n".to_owned(),
                config.log_lines.to_string(),
                "--no-pager".to_owned(),
            ],
        }
    }

    fn service_action(config: &HytaleConfig, action: ServiceAction) -> Self {
        Self {
            program: "sudo",
            args: vec![
                "-n".to_owned(),
                "systemctl".to_owned(),
                action.systemctl_arg().to_owned(),
                config.service_name.clone(),
            ],
        }
    }

    fn systemctl(args: Vec<String>) -> Self {
        Self {
            program: "systemctl",
            args,
        }
    }
}

#[derive(Debug, Clone)]
struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

impl CommandOutput {
    fn combined_output(&self) -> String {
        match (self.stdout.trim(), self.stderr.trim()) {
            ("", "") => "(no output)".to_owned(),
            (stdout, "") => stdout.to_owned(),
            ("", stderr) => stderr.to_owned(),
            (stdout, stderr) => format!("{stdout}\n{stderr}"),
        }
    }
}

impl From<(ExitStatus, Vec<u8>, Vec<u8>)> for CommandOutput {
    fn from((status, stdout, stderr): (ExitStatus, Vec<u8>, Vec<u8>)) -> Self {
        Self {
            success: status.success(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }
}

async fn run_command(
    spec: CommandSpec,
    timeout_duration: Duration,
) -> Result<CommandOutput, Error> {
    let mut command = Command::new(spec.program);
    command.args(&spec.args).kill_on_drop(true);

    let output = timeout(timeout_duration, command.output())
        .await
        .with_context(|| format!("command timed out after {}s", timeout_duration.as_secs()))??;

    Ok((output.status, output.stdout, output.stderr).into())
}

fn code_block(value: &str) -> String {
    format!("```text\n{}\n```", value.replace("```", "'''"))
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

    #[test]
    fn config_uses_defaults_and_required_role() {
        with_env(
            &[
                ("HYTALE_MANAGER_ROLE_ID", Some("12345")),
                ("HYTALE_SERVICE_NAME", None),
                ("HYTALE_LOG_LINES", None),
                ("HYTALE_COMMAND_TIMEOUT_SECONDS", None),
            ],
            || {
                let config = HytaleConfig::from_env().unwrap();

                assert_eq!(config.manager_role_id, RoleId::new(12345));
                assert_eq!(config.service_name, DEFAULT_SERVICE_NAME);
                assert_eq!(config.log_lines, DEFAULT_LOG_LINES);
                assert_eq!(config.timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECONDS));
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
    fn config_clamps_log_lines_and_reads_timeout() {
        with_env(
            &[
                ("HYTALE_MANAGER_ROLE_ID", Some("12345")),
                ("HYTALE_LOG_LINES", Some("999")),
                ("HYTALE_COMMAND_TIMEOUT_SECONDS", Some("22")),
            ],
            || {
                let config = HytaleConfig::from_env().unwrap();

                assert_eq!(config.log_lines, MAX_LOG_LINES);
                assert_eq!(config.timeout, Duration::from_secs(22));
            },
        );
    }

    #[test]
    fn command_specs_are_allowlisted() {
        let config = HytaleConfig {
            manager_role_id: RoleId::new(12345),
            service_name: "hytale-server.service".to_owned(),
            log_lines: 40,
            timeout: Duration::from_secs(15),
        };

        assert_eq!(
            CommandSpec::is_active(&config),
            CommandSpec {
                program: "systemctl",
                args: vec!["is-active".to_owned(), "hytale-server.service".to_owned()],
            }
        );
        assert_eq!(
            CommandSpec::is_enabled(&config),
            CommandSpec {
                program: "systemctl",
                args: vec!["is-enabled".to_owned(), "hytale-server.service".to_owned()],
            }
        );
        assert_eq!(
            CommandSpec::system_status(&config),
            CommandSpec {
                program: "systemctl",
                args: vec![
                    "status".to_owned(),
                    "hytale-server.service".to_owned(),
                    "--no-pager".to_owned()
                ],
            }
        );
        assert_eq!(
            CommandSpec::logs(&config),
            CommandSpec {
                program: "journalctl",
                args: vec![
                    "-u".to_owned(),
                    "hytale-server.service".to_owned(),
                    "-n".to_owned(),
                    "40".to_owned(),
                    "--no-pager".to_owned()
                ],
            }
        );
        assert_eq!(
            CommandSpec::service_action(&config, ServiceAction::Restart),
            CommandSpec {
                program: "sudo",
                args: vec![
                    "-n".to_owned(),
                    "systemctl".to_owned(),
                    "restart".to_owned(),
                    "hytale-server.service".to_owned()
                ],
            }
        );
    }

    #[test]
    fn truncates_long_output() {
        let text = truncate_text("abcdef", 3);

        assert_eq!(text, "abc\n... output trimmed ...");
    }

    #[test]
    fn formats_friendly_status() {
        let report = format_status_report(
            "hytale-server.service",
            "active",
            "enabled",
            "Loaded: loaded\nActive: active (running)",
        );

        assert!(report.contains("The Hytale server is running."));
        assert!(report.contains("set to start automatically"));
        assert!(report.contains("```text"));
    }
}

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    if let Ok(head) = std::fs::read_to_string(".git/HEAD")
        && let Some(ref_path) = head.strip_prefix("ref: ").map(str::trim)
    {
        println!("cargo:rerun-if-changed=.git/{ref_path}");
    }

    set_build_env("BUILD_SOURCE_REF", source_ref());
    set_build_env("BUILD_COMMIT", build_commit());
    set_build_env("BUILD_REPOSITORY", build_repository());
    set_build_env("BUILD_RELEASE_ARCH", build_release_arch());
    set_build_env("BUILD_RELEASE_TAG", build_release_tag());
    println!("cargo:rustc-env=BUILD_INPUT_STATE={}", build_input_state());
}

fn set_build_env(key: &str, value: String) {
    println!("cargo:rustc-env={key}={value}");
}

fn source_ref() -> String {
    env_value("BUILD_SOURCE_REF")
        .or_else(|| env_value("GITHUB_HEAD_REF"))
        .or_else(|| env_value("GITHUB_REF_NAME"))
        .or_else(|| git_output(&["rev-parse", "--abbrev-ref", "HEAD"]).map(normalize_git_ref))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn build_commit() -> String {
    env_value("BUILD_COMMIT")
        .or_else(|| env_value("GITHUB_SHA"))
        .or_else(|| git_output(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn build_repository() -> String {
    env_value("BUILD_REPOSITORY")
        .or_else(|| env_value("GITHUB_REPOSITORY"))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn build_release_tag() -> String {
    env_value("BUILD_RELEASE_TAG").unwrap_or_else(|| "unknown".to_owned())
}

fn build_release_arch() -> String {
    env_value("BUILD_RELEASE_ARCH").unwrap_or_else(|| "unknown".to_owned())
}

fn env_value(key: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={key}");
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

fn git_output(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_git_ref(value: String) -> String {
    if value == "HEAD" {
        "detached".to_owned()
    } else {
        value
    }
}

fn build_input_state() -> &'static str {
    match (has_tracked_changes(), has_untracked_files()) {
        (Some(false), Some(false)) => "clean",
        (Some(_), Some(_)) => "dirty",
        _ => "unknown",
    }
}

fn has_tracked_changes() -> Option<bool> {
    let status = Command::new("git")
        .args([
            "diff-index",
            "--quiet",
            "HEAD",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "src",
        ])
        .status()
        .ok()?;

    match status.code() {
        Some(0) => Some(false),
        Some(1) => Some(true),
        _ => None,
    }
}

fn has_untracked_files() -> Option<bool> {
    let output = Command::new("git")
        .args([
            "ls-files",
            "--others",
            "--exclude-standard",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "build.rs",
            "src",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(!output.stdout.is_empty())
}

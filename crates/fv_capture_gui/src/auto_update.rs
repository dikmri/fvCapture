use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

const REPOSITORY: &str = "dikmri/fvCapture";
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/dikmri/fvCapture/releases/latest";
#[cfg(windows)]
const INSTALL_PS1_URL: &str =
    "https://raw.githubusercontent.com/dikmri/fvCapture/main/scripts/install.ps1";
#[cfg(any(target_os = "linux", target_os = "macos"))]
const INSTALL_SH_URL: &str =
    "https://raw.githubusercontent.com/dikmri/fvCapture/main/scripts/install.sh";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub release_url: String,
    pub release_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
}

pub fn spawn_update_check() -> Receiver<Result<Option<UpdateInfo>, String>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = check_for_update().map_err(|error| error.to_string());
        let _ = tx.send(result);
    });
    rx
}

pub fn check_for_update() -> Result<Option<UpdateInfo>> {
    let asset =
        platform_asset_name().context("this platform is not supported by fvCapture releases")?;
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut response = agent
        .get(LATEST_RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header(
            "User-Agent",
            &format!("fvCapture/{CURRENT_VERSION} ({REPOSITORY})"),
        )
        .call()
        .context("failed to query GitHub Releases")?;
    let body = response
        .body_mut()
        .read_to_string()
        .context("failed to read GitHub Releases response")?;
    let release: GitHubRelease =
        serde_json::from_str(&body).context("failed to parse GitHub Releases response")?;
    Ok(update_from_release(&release, CURRENT_VERSION, asset))
}

pub fn launch_updater(info: &UpdateInfo) -> Result<()> {
    let exe = env::current_exe().context("failed to locate current executable")?;
    let install_dir = exe
        .parent()
        .map(Path::to_path_buf)
        .context("failed to locate current install directory")?;

    if cfg!(windows) {
        launch_windows_updater(info, &install_dir)
    } else if cfg!(any(target_os = "linux", target_os = "macos")) {
        launch_unix_updater(info, &install_dir)
    } else {
        bail!("automatic updates are not supported on this platform");
    }
}

fn update_from_release(
    release: &GitHubRelease,
    current_version: &str,
    platform_asset: &str,
) -> Option<UpdateInfo> {
    if !is_newer_version(&release.tag_name, current_version) {
        return None;
    }

    let has_platform_asset = release
        .assets
        .iter()
        .any(|asset| asset.name == platform_asset);
    if !has_platform_asset {
        return None;
    }

    Some(UpdateInfo {
        version: release.tag_name.clone(),
        release_url: release.html_url.clone(),
        release_notes: release.body.clone().filter(|body| !body.trim().is_empty()),
    })
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    let Ok(candidate) = parse_version(candidate) else {
        return false;
    };
    let Ok(current) = parse_version(current) else {
        return false;
    };
    candidate > current
}

fn parse_version(value: &str) -> Result<Version, semver::Error> {
    Version::parse(value.trim().trim_start_matches('v'))
}

fn platform_asset_name() -> Option<&'static str> {
    match env::consts::OS {
        "windows" => Some("fvCapture-windows-x86_64.zip"),
        "macos" => Some("fvCapture-macos.tar.gz"),
        "linux" => Some("fvCapture-linux-x86_64.tar.gz"),
        _ => None,
    }
}

#[cfg(windows)]
fn launch_windows_updater(info: &UpdateInfo, install_dir: &Path) -> Result<()> {
    let script_path = temp_script_path("ps1");
    let script = format!(
        r#"$ErrorActionPreference = "Stop"
$pidToWait = {pid}
$installDir = '{install_dir}'
$version = '{version}'
$log = Join-Path ([IO.Path]::GetTempPath()) "fvCapture-update.log"
try {{
    Wait-Process -Id $pidToWait -ErrorAction SilentlyContinue
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $script = Invoke-RestMethod -Uri '{install_url}' -UseBasicParsing
    $block = [ScriptBlock]::Create($script)
    & $block -Version $version -InstallDir $installDir -NoShortcut -NoPath
    $appPath = Join-Path $installDir "fvCapture.exe"
    if (Test-Path $appPath) {{
        Start-Process -FilePath $appPath -WorkingDirectory $installDir
    }}
}} catch {{
    $_ | Out-File -FilePath $log -Append -Encoding utf8
}} finally {{
    Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
}}
"#,
        pid = std::process::id(),
        install_dir = ps_single_quote(install_dir.display().to_string()),
        version = ps_single_quote(&info.version),
        install_url = INSTALL_PS1_URL,
    );
    fs::write(&script_path, script).with_context(|| {
        format!(
            "failed to write temporary updater script: {}",
            script_path.display()
        )
    })?;

    let mut command = Command::new("powershell");
    command
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path);
    suppress_console_window(&mut command);
    command
        .spawn()
        .context("failed to launch Windows updater process")?;
    Ok(())
}

#[cfg(not(windows))]
fn launch_windows_updater(_info: &UpdateInfo, _install_dir: &Path) -> Result<()> {
    bail!("Windows updater cannot run on this platform")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn launch_unix_updater(info: &UpdateInfo, install_dir: &Path) -> Result<()> {
    let script_path = temp_script_path("sh");
    let script = format!(
        r#"#!/usr/bin/env sh
set -eu
pid={pid}
version={version}
install_dir={install_dir}
install_url={install_url}

while kill -0 "$pid" 2>/dev/null; do
    sleep 0.2
done

if command -v curl >/dev/null 2>&1; then
    FVCAPTURE_VERSION="$version" FVCAPTURE_INSTALL_DIR="$install_dir" FVCAPTURE_SKIP_LINKS=1 sh -c "curl -fsSL \"$install_url\" | sh"
elif command -v wget >/dev/null 2>&1; then
    FVCAPTURE_VERSION="$version" FVCAPTURE_INSTALL_DIR="$install_dir" FVCAPTURE_SKIP_LINKS=1 sh -c "wget -O - \"$install_url\" | sh"
else
    exit 1
fi

if [ -x "$install_dir/fvCapture" ]; then
    nohup "$install_dir/fvCapture" >/dev/null 2>&1 &
fi
rm -f "$0"
"#,
        pid = std::process::id(),
        version = sh_single_quote(&info.version),
        install_dir = sh_single_quote(&install_dir.display().to_string()),
        install_url = sh_single_quote(INSTALL_SH_URL),
    );
    fs::write(&script_path, script).with_context(|| {
        format!(
            "failed to write temporary updater script: {}",
            script_path.display()
        )
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script_path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script_path, permissions)?;
    }

    Command::new("sh")
        .arg(&script_path)
        .spawn()
        .context("failed to launch updater process")?;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn launch_unix_updater(_info: &UpdateInfo, _install_dir: &Path) -> Result<()> {
    bail!("Unix updater cannot run on this platform")
}

fn temp_script_path(extension: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "fvCapture-update-{}.{}",
        std::process::id(),
        extension
    ))
}

#[cfg(windows)]
fn ps_single_quote(value: impl AsRef<str>) -> String {
    value.as_ref().replace('\'', "''")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sh_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag_name: &str, assets: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag_name.to_string(),
            html_url: format!("https://github.com/{REPOSITORY}/releases/tag/{tag_name}"),
            body: Some("- test".to_string()),
            assets: assets
                .iter()
                .map(|name| GitHubAsset {
                    name: (*name).to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn detects_newer_semver_tags() {
        assert!(is_newer_version("v0.4.0", "0.3.0"));
        assert!(!is_newer_version("v0.3.0", "0.3.0"));
        assert!(!is_newer_version("not-a-version", "0.3.0"));
    }

    #[test]
    fn builds_update_info_only_when_platform_asset_exists() {
        let info = update_from_release(
            &release("v0.4.0", &["fvCapture-windows-x86_64.zip"]),
            "0.3.0",
            "fvCapture-windows-x86_64.zip",
        );
        assert_eq!(info.unwrap().version, "v0.4.0");

        let missing_asset = update_from_release(
            &release("v0.4.0", &["fvCapture-linux-x86_64.tar.gz"]),
            "0.3.0",
            "fvCapture-windows-x86_64.zip",
        );
        assert!(missing_asset.is_none());
    }
}

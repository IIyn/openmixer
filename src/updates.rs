//! Updates from the GitHub releases of the project.
//!
//! How an update is installed depends on how OpenMixer was installed:
//! - AppImage: the `.AppImage` file is replaced in place;
//! - standalone binary (tarball, `cargo build`): the binary replaces itself;
//! - distribution package (.deb/.rpm/AUR, under `/usr`): the package manager
//!   owns the files, so we only point to the release.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub const REPO_OWNER: &str = "IIyn";
pub const REPO_NAME: &str = "openmixer";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Target triple in the name of the release tarball.
const TARGET: &str = "x86_64-unknown-linux-gnu";
const APPIMAGE_SUFFIX: &str = "x86_64.AppImage";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub version: String,
    pub name: String,
    pub date: String,
    pub notes: String,
    pub url: String,
    appimage_url: Option<String>,
    has_tarball: bool,
}

impl ReleaseInfo {
    pub fn is_newer(&self) -> bool {
        self_update::version::bump_is_greater(CURRENT_VERSION, &self.version).unwrap_or(false)
    }

    /// `2026-11-06T12:00:00Z` -> `2026-11-06`.
    pub fn day(&self) -> &str {
        self.date.split('T').next().unwrap_or(&self.date)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallKind {
    AppImage(PathBuf),
    Package,
    Standalone,
}

impl InstallKind {
    pub fn detect() -> Self {
        if let Some(path) = std::env::var_os("APPIMAGE") {
            return Self::AppImage(PathBuf::from(path));
        }
        match std::env::current_exe() {
            Ok(exe) if exe.starts_with("/usr") || exe.starts_with("/opt") => Self::Package,
            _ => Self::Standalone,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::AppImage(_) => "AppImage",
            Self::Package => "distribution package",
            Self::Standalone => "standalone binary",
        }
    }

    pub fn can_self_update(&self) -> bool {
        !matches!(self, Self::Package)
    }
}

fn release_page(version: &str) -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/tag/v{version}")
}

/// Releases newest first, with their notes.
pub fn fetch_releases() -> Result<Vec<ReleaseInfo>> {
    let list = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .context("configuring the release list")?
        .fetch()
        .context("fetching releases from GitHub")?;
    Ok(list
        .all()
        .iter()
        .map(|r| {
            let version = r.version().trim_start_matches('v').to_owned();
            ReleaseInfo {
                name: r.name().to_owned(),
                date: r.date().to_owned(),
                notes: r.body().unwrap_or("").to_owned(),
                url: r
                    .release_notes_url()
                    .map_or_else(|| release_page(&version), str::to_owned),
                appimage_url: r
                    .assets()
                    .iter()
                    .find(|a| a.name().ends_with(APPIMAGE_SUFFIX))
                    .map(|a| a.download_url().to_owned()),
                has_tarball: r.has_target_asset(TARGET),
                version,
            }
        })
        .collect())
}

/// Installs `release`. The running program keeps its old code until it is
/// restarted.
pub fn install(release: &ReleaseInfo, kind: &InstallKind) -> Result<()> {
    match kind {
        InstallKind::Package => {
            bail!("OpenMixer was installed by your package manager: update it from there")
        }
        InstallKind::AppImage(path) => {
            let url = release
                .appimage_url
                .as_deref()
                .context("this release has no AppImage")?;
            replace_appimage(url, path)
        }
        InstallKind::Standalone => {
            if !release.has_tarball {
                bail!("this release has no {TARGET} archive");
            }
            self_update::backends::github::Update::configure()
                .repo_owner(REPO_OWNER)
                .repo_name(REPO_NAME)
                .bin_name("openmixer")
                .target(TARGET)
                .current_version(CURRENT_VERSION)
                .release_tag(format!("v{}", release.version))
                .show_download_progress(false)
                .show_output(false)
                .no_confirm(true)
                .build()
                .context("configuring the update")?
                .update()
                .context("installing the update")?;
            Ok(())
        }
    }
}

fn replace_appimage(url: &str, path: &PathBuf) -> Result<()> {
    let tmp = path.with_extension("AppImage.download");
    let file = fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let result = self_update::Download::from_url(url)
        .request_header("Accept", "application/octet-stream")
        .timeout(DOWNLOAD_TIMEOUT)
        .show_download_progress(false)
        .download_to(file)
        .context("downloading the AppImage");
    if let Err(e) = result {
        fs::remove_file(&tmp).ok();
        return Err(e);
    }
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("making {} executable", tmp.display()))?;
    // The old file stays mapped by the running instance; rename is atomic.
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

pub fn open_in_browser(url: &str) -> Result<()> {
    Command::new("xdg-open")
        .arg(url)
        .spawn()
        .context("running xdg-open")?;
    Ok(())
}

/// One line of release notes, with light Markdown handling.
#[derive(Debug, PartialEq, Eq)]
pub enum NoteLine {
    Heading(String),
    Bullet(String),
    Text(String),
    Blank,
}

fn strip_inline(s: &str) -> String {
    s.replace("**", "").replace('`', "")
}

pub fn parse_notes(notes: &str) -> Vec<NoteLine> {
    let mut out = Vec::new();
    for line in notes.lines() {
        let t = line.trim();
        let parsed = if t.is_empty() {
            NoteLine::Blank
        } else if let Some(h) = t.strip_prefix('#') {
            NoteLine::Heading(strip_inline(h.trim_start_matches('#').trim()))
        } else if let Some(b) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            NoteLine::Bullet(strip_inline(b))
        } else {
            NoteLine::Text(strip_inline(t))
        };
        // Collapse runs of blank lines.
        if parsed == NoteLine::Blank && matches!(out.last(), None | Some(NoteLine::Blank)) {
            continue;
        }
        out.push(parsed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_markdown() {
        let notes = "## What's new\n\n\n- **Tray** icon\n* Fix `meters`\nPlain text";
        assert_eq!(
            parse_notes(notes),
            vec![
                NoteLine::Heading("What's new".into()),
                NoteLine::Blank,
                NoteLine::Bullet("Tray icon".into()),
                NoteLine::Bullet("Fix meters".into()),
                NoteLine::Text("Plain text".into()),
            ]
        );
    }

    #[test]
    fn version_comparison() {
        let r = |v: &str| ReleaseInfo {
            version: v.into(),
            name: String::new(),
            date: "2026-11-06T10:00:00Z".into(),
            notes: String::new(),
            url: String::new(),
            appimage_url: None,
            has_tarball: false,
        };
        assert!(r("99.0.0").is_newer());
        assert!(!r(CURRENT_VERSION).is_newer());
        assert!(!r("not-a-version").is_newer());
        assert_eq!(r("1.0.0").day(), "2026-11-06");
    }
}

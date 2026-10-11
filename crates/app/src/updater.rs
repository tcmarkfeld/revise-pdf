//! Update checks against the project's GitHub releases, and in-place install
//! of a newer build. A release's disk image is mounted, its app must pass
//! Gatekeeper and be signed by the same team as the running app, and only
//! then replaces the running bundle. Uses the system `curl`, `hdiutil`,
//! `codesign`, `spctl` and `ditto`, so no networking crates are needed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, SystemTime};

use egui::RichText;

use crate::theme;
use crate::toolbar::primary_button;

const LATEST: &str = "https://api.github.com/repos/tcmarkfeld/revise-pdf/releases/latest";
const RELEASES: &str = "https://github.com/tcmarkfeld/revise-pdf/releases";
const CURRENT: &str = env!("CARGO_PKG_VERSION");
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
pub struct Release {
    version: String,
    page: String,
    dmg: Option<String>,
}

enum Msg {
    Available(Release),
    UpToDate,
    Installed,
    Failed(String),
}

enum Dialog {
    Available(Release),
    Installing,
    UpToDate,
    Failed(String),
}

#[derive(Default)]
pub struct Updater {
    rx: Option<Receiver<Msg>>,
    /// The user asked (Check for Updates…), so "up to date" and errors are
    /// shown too; automatic checks only speak up when there is an update.
    manual: bool,
    dialog: Option<Dialog>,
}

impl Updater {
    /// Checks in the background if the last check was over a day ago.
    pub fn check_daily(&mut self, ctx: &egui::Context) {
        let stamp = stamp_file();
        let recent = stamp
            .as_ref()
            .and_then(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
            .is_some_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < CHECK_EVERY);
        if !recent {
            self.start(ctx, false);
        }
    }

    pub fn check_now(&mut self, ctx: &egui::Context) {
        self.start(ctx, true);
    }

    fn start(&mut self, ctx: &egui::Context, manual: bool) {
        if self.rx.is_some() {
            self.manual |= manual;
            return;
        }
        if let Some(f) = stamp_file() {
            let _ = std::fs::write(f, CURRENT);
        }
        self.manual = manual;
        self.spawn(ctx, || match latest() {
            Ok(r) if is_newer(&r.version, CURRENT) => Msg::Available(r),
            Ok(_) => Msg::UpToDate,
            Err(e) => Msg::Failed(e),
        });
    }

    fn spawn(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Msg + Send + 'static) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
        self.rx = Some(rx);
    }

    /// Shows the update dialog. Returns true once a new version is installed
    /// and the app should quit (it relaunches itself after exiting).
    pub fn ui(&mut self, ctx: &egui::Context) -> bool {
        if let Some(msg) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.rx = None;
            match msg {
                Msg::Available(r) => self.dialog = Some(Dialog::Available(r)),
                Msg::UpToDate if self.manual => self.dialog = Some(Dialog::UpToDate),
                Msg::Failed(e) if self.manual || matches!(self.dialog, Some(Dialog::Installing)) => self.dialog = Some(Dialog::Failed(e)),
                Msg::Installed => {
                    self.dialog = None;
                    if let Some(bundle) = running_bundle() {
                        relaunch_after_exit(&bundle);
                    }
                    return true;
                }
                _ => {}
            }
        }
        let Some(dialog) = &self.dialog else { return false };
        let p = theme::palette(ctx);
        let (title, body) = match dialog {
            Dialog::Available(r) => (format!("Revise {} is available", r.version), format!("You have version {CURRENT}. Your open document stays as it is until you restart.")),
            Dialog::Installing => ("Updating Revise…".to_string(), "Downloading and verifying the new version.".to_string()),
            Dialog::UpToDate => ("You’re up to date".to_string(), format!("Revise {CURRENT} is the latest version.")),
            Dialog::Failed(e) => ("Couldn’t update Revise".to_string(), format!("{e}\nYou can download the latest version from GitHub instead.")),
        };
        let mut install = None;
        let mut close = false;
        egui::Modal::new(egui::Id::new("update")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(title).size(15.0).strong());
            ui.add_space(6.0);
            ui.label(RichText::new(body).size(13.0).color(p.text_muted));
            ui.add_space(16.0);
            ui.horizontal(|ui| match dialog {
                Dialog::Available(r) => {
                    if ui.button("Release Notes").clicked() {
                        open_url(&r.page);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if running_bundle().is_some() && r.dmg.is_some() {
                            if ui.add(primary_button("Install and Relaunch")).clicked() {
                                install = Some(r.clone());
                            }
                        } else if ui.add(primary_button("Download")).clicked() {
                            open_url(&r.page);
                            close = true;
                        }
                        if ui.button("Later").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Installing => {
                    ui.spinner();
                }
                Dialog::UpToDate => {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(primary_button("OK")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Failed(_) => {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(primary_button("Download")).clicked() {
                            open_url(RELEASES);
                            close = true;
                        }
                        if ui.button("OK").clicked() {
                            close = true;
                        }
                    });
                }
            });
        });
        if close {
            self.dialog = None;
        }
        if let Some(r) = install {
            self.dialog = Some(Dialog::Installing);
            self.spawn(ctx, move || install_release(&r).map_or_else(Msg::Failed, |()| Msg::Installed));
        }
        false
    }
}

fn stamp_file() -> Option<PathBuf> {
    Some(crate::persist::support_dir("Updates")?.join("last-check"))
}

fn latest() -> Result<Release, String> {
    let out = run(
        "curl",
        ["-fsSL", "--max-time", "15", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: Revise", LATEST].map(OsStr::new),
    )
    .map_err(|_| "Couldn’t reach GitHub to check for updates.".to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&out).map_err(|e| e.to_string())?;
    let tag = json["tag_name"].as_str().ok_or("The latest release has no version tag.")?;
    let dmg = json["assets"].as_array().and_then(|assets| {
        assets
            .iter()
            .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(".dmg")))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(str::to_string)
    });
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        page: json["html_url"].as_str().unwrap_or(RELEASES).to_string(),
        dmg,
    })
}

/// Whether dotted version `a` is later than `b` ("0.10.0" > "0.9.1").
fn is_newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        let mut p: Vec<u64> = v.split('.').map(|s| s.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect();
        p.resize(3, 0);
        p
    };
    parts(a) > parts(b)
}

/// The `.app` bundle this process runs from, if any.
fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.parent()?.parent()?.parent()?.to_path_buf();
    bundle.extension().is_some_and(|e| e == "app").then_some(bundle)
}

fn install_release(r: &Release) -> Result<(), String> {
    let bundle = running_bundle().ok_or("Revise isn’t running from an app bundle.")?;
    let url = r.dmg.as_deref().ok_or("This release has no disk image.")?;
    let dir = std::env::temp_dir().join(format!("revise-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dmg = dir.join("Revise.dmg");
    let mount = dir.join("volume");
    let result = (|| {
        run("curl", [OsStr::new("-fsSL"), OsStr::new("-o"), dmg.as_os_str(), OsStr::new(url)]).map_err(|_| "The download failed.".to_string())?;
        run("hdiutil", [OsStr::new("attach"), OsStr::new("-nobrowse"), OsStr::new("-readonly"), OsStr::new("-mountpoint"), mount.as_os_str(), dmg.as_os_str()])?;
        let new = mount.join("Revise.app");
        verify(&new, &bundle)?;
        replace(&new, &bundle)
    })();
    let _ = run("hdiutil", [OsStr::new("detach"), OsStr::new("-force"), mount.as_os_str()]);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The new app must be intact, accepted by Gatekeeper (notarized), and signed
/// by the same team as the running one.
fn verify(new: &Path, current: &Path) -> Result<(), String> {
    let untrusted = |_| "The downloaded update isn’t correctly signed, so it wasn’t installed.".to_string();
    run("codesign", [OsStr::new("--verify"), OsStr::new("--strict"), new.as_os_str()]).map_err(untrusted)?;
    run("spctl", [OsStr::new("--assess"), OsStr::new("--type"), OsStr::new("exec"), new.as_os_str()]).map_err(untrusted)?;
    let new_team = team(new).ok_or_else(|| untrusted(String::new()))?;
    match team(current) {
        Some(t) if t != new_team => Err(untrusted(String::new())),
        _ => Ok(()),
    }
}

fn team(app: &Path) -> Option<String> {
    let out = Command::new("codesign").arg("-dv").arg("--verbose=2").arg(app).output().ok()?;
    let info = String::from_utf8_lossy(&out.stderr);
    let id = info.lines().find_map(|l| l.strip_prefix("TeamIdentifier="))?.trim();
    (id != "not set").then(|| id.to_string())
}

/// Swaps `new` in for the bundle at `current`, restoring it on failure.
fn replace(new: &Path, current: &Path) -> Result<(), String> {
    let parent = current.parent().ok_or("Unexpected app location.")?;
    let staged = parent.join(".Revise-update.app");
    let old = parent.join(".Revise-old.app");
    let _ = std::fs::remove_dir_all(&staged);
    let _ = std::fs::remove_dir_all(&old);
    let denied = |_| format!("Revise doesn’t have permission to replace itself in {}.", parent.display());
    run("ditto", [new.as_os_str(), staged.as_os_str()]).map_err(denied)?;
    std::fs::rename(current, &old).map_err(|e| denied(e.to_string()))?;
    if let Err(e) = std::fs::rename(&staged, current) {
        let _ = std::fs::rename(&old, current);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Opens `bundle` again once this process has exited.
fn relaunch_after_exit(bundle: &Path) {
    let _ = Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; open "$2""#)
        .arg("sh")
        .arg(std::process::id().to_string())
        .arg(bundle)
        .spawn();
}

fn open_url(url: &str) {
    let _ = Command::new("open").arg(url).spawn();
}

/// Runs a command, returning its stdout or its stderr as the error.
fn run<I: IntoIterator<Item = S>, S: AsRef<OsStr>>(cmd: &str, args: I) -> Result<Vec<u8>, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(out.stdout) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()) }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.1"));
    }
}

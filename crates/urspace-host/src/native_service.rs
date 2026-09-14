use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use rand::Rng as _;

const LABEL_PREFIX: &str = "online.urspace.site";

pub struct InstallOutcome {
    pub definition_path: PathBuf,
    pub persistence_note: Option<String>,
}

pub fn ensure_supported() -> Result<()> {
    if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
        Ok(())
    } else {
        bail!("automatic services currently support macOS and Linux")
    }
}

pub fn is_installed(name: &str) -> Result<bool> {
    if !cfg!(target_os = "macos") && !cfg!(target_os = "linux") {
        return Ok(false);
    }
    Ok(definition_path(name)?.exists())
}

pub fn install(
    name: &str,
    executable: &Path,
    data_directory: &Path,
    _service_directory: &Path,
) -> Result<InstallOutcome> {
    ensure_supported()?;
    let definition_path = definition_path(name)?;
    #[cfg(target_os = "macos")]
    {
        let label = service_label(name);
        let domain = launchd_domain()?;
        let target = format!("{domain}/{label}");
        let definition = render_launchd_plist(
            &label,
            executable,
            name,
            data_directory,
            &_service_directory.join("service.stdout.log"),
            &_service_directory.join("service.stderr.log"),
        );
        write_definition(&definition_path, definition.as_bytes())?;
        launchd_bootout_if_loaded(&target)?;
        run_checked(launchctl(), ["enable", target.as_str()])?;
        run_checked(
            launchctl(),
            [
                "bootstrap",
                domain.as_str(),
                definition_path
                    .to_str()
                    .context("launchd definition path is not valid UTF-8")?,
            ],
        )?;
        Ok(InstallOutcome {
            definition_path,
            persistence_note: Some(
                "launchd starts this service when you log in and restarts it after failures".into(),
            ),
        })
    }
    #[cfg(target_os = "linux")]
    {
        let unit = unit_name(name);
        let definition = render_systemd_unit(executable, name, data_directory);
        write_definition(&definition_path, definition.as_bytes())?;
        run_checked(systemctl()?, ["--user", "daemon-reload"])?;
        run_checked(systemctl()?, ["--user", "enable", unit.as_str()])?;
        run_checked(systemctl()?, ["--user", "restart", unit.as_str()])?;
        Ok(InstallOutcome {
            definition_path,
            persistence_note: linux_persistence_note(),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        bail!("automatic services currently support macOS and Linux")
    }
}

pub fn start(name: &str) -> Result<()> {
    require_definition(name)?;
    #[cfg(target_os = "macos")]
    {
        let domain = launchd_domain()?;
        let target = format!("{domain}/{}", service_label(name));
        if run_output(launchctl(), ["kickstart", target.as_str()])?
            .status
            .success()
        {
            return Ok(());
        }
        run_checked(launchctl(), ["enable", target.as_str()])?;
        run_checked(
            launchctl(),
            [
                "bootstrap",
                domain.as_str(),
                definition_path(name)?
                    .to_str()
                    .context("launchd definition path is not valid UTF-8")?,
            ],
        )
    }
    #[cfg(target_os = "linux")]
    {
        run_checked(
            systemctl()?,
            ["--user", "enable", "--now", unit_name(name).as_str()],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        ensure_supported()
    }
}

pub fn stop(name: &str) -> Result<()> {
    require_definition(name)?;
    #[cfg(target_os = "macos")]
    {
        launchd_bootout_if_loaded(&format!("{}/{}", launchd_domain()?, service_label(name)))
    }
    #[cfg(target_os = "linux")]
    {
        run_checked(systemctl()?, ["--user", "stop", unit_name(name).as_str()])
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        ensure_supported()
    }
}

pub fn restart(name: &str) -> Result<()> {
    require_definition(name)?;
    #[cfg(target_os = "macos")]
    {
        let domain = launchd_domain()?;
        let target = format!("{domain}/{}", service_label(name));
        if run_output(launchctl(), ["kickstart", "-k", target.as_str()])?
            .status
            .success()
        {
            return Ok(());
        }
        start(name)
    }
    #[cfg(target_os = "linux")]
    {
        run_checked(
            systemctl()?,
            ["--user", "restart", unit_name(name).as_str()],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        ensure_supported()
    }
}

pub fn uninstall(name: &str) -> Result<PathBuf> {
    let path = require_definition(name)?;
    #[cfg(target_os = "macos")]
    {
        let target = format!("{}/{}", launchd_domain()?, service_label(name));
        launchd_bootout_if_loaded(&target)?;
    }
    #[cfg(target_os = "linux")]
    {
        let unit = unit_name(name);
        let _ = run_output(systemctl()?, ["--user", "disable", "--now", unit.as_str()]);
    }
    std::fs::remove_file(&path)
        .with_context(|| format!("remove service definition {}", path.display()))?;
    #[cfg(target_os = "linux")]
    run_checked(systemctl()?, ["--user", "daemon-reload"])?;
    Ok(path)
}

fn require_definition(name: &str) -> Result<PathBuf> {
    ensure_supported()?;
    let path = definition_path(name)?;
    if !path.exists() {
        bail!("service `{name}` is not installed; run `urspace service install` first");
    }
    Ok(path)
}

fn service_label(name: &str) -> String {
    format!("{LABEL_PREFIX}.{name}")
}

fn unit_name(name: &str) -> String {
    format!("urspace-{name}.service")
}

fn definition_path(name: &str) -> Result<PathBuf> {
    let home = dirs::home_dir().context("home directory is unavailable")?;
    if cfg!(target_os = "macos") {
        Ok(home
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", service_label(name))))
    } else if cfg!(target_os = "linux") {
        Ok(dirs::config_dir()
            .unwrap_or_else(|| home.join(".config"))
            .join("systemd/user")
            .join(unit_name(name)))
    } else {
        bail!("automatic services currently support macOS and Linux")
    }
}

#[cfg(target_os = "macos")]
fn launchd_domain() -> Result<String> {
    let output = run_output(Path::new("/usr/bin/id"), ["-u"])?;
    if !output.status.success() {
        bail!("could not determine the current user id");
    }
    let uid = String::from_utf8(output.stdout)
        .context("current user id is not UTF-8")?
        .trim()
        .to_owned();
    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("current user id is invalid");
    }
    Ok(format!("gui/{uid}"))
}

#[cfg(target_os = "macos")]
fn launchctl() -> &'static Path {
    Path::new("/bin/launchctl")
}

#[cfg(target_os = "macos")]
fn launchd_bootout_if_loaded(target: &str) -> Result<()> {
    let output = run_output(launchctl(), ["bootout", target])?;
    if output.status.success() || output.status.code() == Some(3) {
        return Ok(());
    }
    check_output(launchctl(), output)
}

#[cfg(target_os = "linux")]
fn systemctl() -> Result<&'static Path> {
    ["/usr/bin/systemctl", "/bin/systemctl"]
        .into_iter()
        .map(Path::new)
        .find(|path| path.is_file())
        .context("systemctl is required for automatic services on Linux")
}

#[cfg(target_os = "linux")]
fn linux_persistence_note() -> Option<String> {
    let loginctl = ["/usr/bin/loginctl", "/bin/loginctl"]
        .into_iter()
        .map(Path::new)
        .find(|path| path.is_file())?;
    let output = run_output(loginctl, ["show-user", "--property=Linger", "--value"]).ok()?;
    let linger = String::from_utf8_lossy(&output.stdout);
    (!linger.trim().eq_ignore_ascii_case("yes")).then(|| {
        "systemd will start this at login; run `loginctl enable-linger` if it must start at boot before login".into()
    })
}

fn run_checked<'a>(program: &Path, args: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let output = run_output(program, args)?;
    check_output(program, output)
}

fn check_output(program: &Path, output: Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let mut message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    message.truncate(4_096);
    if message.is_empty() {
        message = format!("exited with {}", output.status);
    }
    bail!("{} failed: {message}", program.display())
}

fn run_output<'a>(program: &Path, args: impl IntoIterator<Item = &'a str>) -> Result<Output> {
    Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("run {}", program.display()))
}

fn write_definition(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().context("service definition has no parent")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create service definition directory {}", parent.display()))?;
    let temporary = path.with_extension(format!("tmp-{}", rand::rng().random::<u64>()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("create service definition {}", temporary.display()))?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)
            .with_context(|| format!("install service definition {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(any(target_os = "macos", test))]
fn render_launchd_plist(
    label: &str,
    executable: &Path,
    name: &str,
    data_directory: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
) -> String {
    let string = |value: &str| format!("<string>{}</string>", xml_escape(value));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
  <key>Label</key>\n  {}\n\
  <key>ProgramArguments</key>\n  <array>\n    {}\n    <string>service</string>\n    <string>managed-run</string>\n    {}\n  </array>\n\
  <key>EnvironmentVariables</key>\n  <dict>\n    <key>URSPACE_DATA_DIR</key>\n    {}\n  </dict>\n\
  <key>RunAtLoad</key>\n  <true/>\n\
  <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
  <key>ThrottleInterval</key>\n  <integer>5</integer>\n\
  <key>Umask</key>\n  <integer>63</integer>\n\
  <key>StandardOutPath</key>\n  {}\n\
  <key>StandardErrorPath</key>\n  {}\n\
</dict>\n</plist>\n",
        string(label),
        string(&executable.to_string_lossy()),
        string(name),
        string(&data_directory.to_string_lossy()),
        string(&stdout_path.to_string_lossy()),
        string(&stderr_path.to_string_lossy()),
    )
}

#[cfg(any(target_os = "linux", test))]
fn render_systemd_unit(executable: &Path, name: &str, data_directory: &Path) -> String {
    let data_directory_text = data_directory.to_string_lossy();
    let executable = systemd_quote(&executable.to_string_lossy());
    let data_directory = systemd_quote(&data_directory_text);
    let environment = systemd_quote(&format!("URSPACE_DATA_DIR={data_directory_text}"));
    format!(
        "[Unit]\n\
Description=Urspace site {name}\n\
Wants=network-online.target\n\
After=network-online.target\n\n\
[Service]\n\
Type=simple\n\
ExecStart={executable} \"service\" \"managed-run\" \"{name}\"\n\
Environment={environment}\n\
Restart=on-failure\n\
RestartSec=5s\n\
UMask=0077\n\
NoNewPrivileges=true\n\
PrivateTmp=true\n\
ProtectSystem=strict\n\
ProtectHome=read-only\n\
ReadWritePaths={data_directory}\n\n\
[Install]\n\
WantedBy=default.target\n"
    )
}

#[cfg(any(target_os = "macos", test))]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(target_os = "linux", test))]
fn systemd_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchd_definition_uses_argument_array_and_escapes_paths() {
        let plist = render_launchd_plist(
            "online.urspace.site.demo",
            Path::new("/Applications/A&B/urspace"),
            "demo",
            Path::new("/Users/demo/A&B"),
            Path::new("/tmp/out"),
            Path::new("/tmp/err"),
        );
        assert!(plist.contains("<string>/Applications/A&amp;B/urspace</string>"));
        assert!(plist.contains("<string>managed-run</string>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
        assert!(!plist.contains("A&B"));
    }

    #[test]
    fn systemd_definition_quotes_specifiers_and_shell_syntax() {
        let unit = render_systemd_unit(
            Path::new("/home/me/100%/$bin/urspace"),
            "demo",
            Path::new("/home/me/urspace data"),
        );
        assert!(unit.contains("100%%/$$bin/urspace"));
        assert!(unit.contains("ExecStart=\""));
        assert!(unit.contains("Environment=\"URSPACE_DATA_DIR=/home/me/urspace data\""));
        assert!(unit.contains("ReadWritePaths=\"/home/me/urspace data\""));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("ProtectSystem=strict"));
    }

    #[test]
    fn supervisor_definition_replacement_is_atomic_and_private() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("service-definition");
        write_definition(&path, b"first").unwrap();
        write_definition(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

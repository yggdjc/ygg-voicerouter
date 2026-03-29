//! `voicerouter service` — systemd user service management.

use anyhow::{Context, Result};

const SERVICE_NAME: &str = "voicerouter";
const SERVICE_UNIT: &str = "voicerouter.service";
const OVERLAY_SERVICE_NAME: &str = "voicerouter-overlay";
const OVERLAY_SERVICE_UNIT: &str = "voicerouter-overlay.service";

pub fn run(action: &str) -> Result<()> {
    match action {
        "install" => install(),
        "uninstall" => uninstall(),
        "start" | "stop" | "status" | "restart" => systemctl(action),
        other => {
            anyhow::bail!(
                "unknown service action {other:?}. \
                 Valid: install, uninstall, start, stop, restart, status"
            );
        }
    }
}

fn install() -> Result<()> {
    let unit_dir = unit_dir()?;
    std::fs::create_dir_all(&unit_dir).with_context(|| {
        format!("creating unit dir: {}", unit_dir.display())
    })?;

    let binary =
        std::env::current_exe().context("cannot determine binary path")?;
    let lib_dir = binary.parent().map(|p| p.join("deps"));
    let env_line = if let Some(ref dir) = lib_dir {
        if dir.exists() {
            format!("Environment=LD_LIBRARY_PATH={}\n", dir.display())
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let unit_content = format!(
        "[Unit]\n\
         Description=voicerouter — offline voice router\n\
         After=graphical-session.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={binary}\n\
         {env_line}\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        binary = binary.display(),
    );

    let unit_path = unit_dir.join(SERVICE_UNIT);
    std::fs::write(&unit_path, unit_content).with_context(|| {
        format!("writing service file: {}", unit_path.display())
    })?;

    println!("Installed: {}", unit_path.display());

    // Install overlay service if the overlay binary exists alongside the daemon.
    let overlay_binary = binary.with_file_name("voicerouter-overlay");
    if overlay_binary.exists() {
        let overlay_unit_content = format!(
            "[Unit]\n\
             Description=voicerouter-overlay — visual feedback overlay\n\
             BindsTo=voicerouter.service\n\
             After=voicerouter.service\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={binary}\n\
             Restart=on-failure\n\
             RestartSec=3\n\
             \n\
             [Install]\n\
             WantedBy=voicerouter.service\n",
            binary = overlay_binary.display(),
        );

        let overlay_unit_path = unit_dir.join(OVERLAY_SERVICE_UNIT);
        std::fs::write(&overlay_unit_path, &overlay_unit_content).with_context(|| {
            format!("writing overlay service file: {}", overlay_unit_path.display())
        })?;
        println!("Installed: {}", overlay_unit_path.display());
        run_systemctl(&["--user", "enable", OVERLAY_SERVICE_NAME])?;
    }

    run_systemctl(&["--user", "daemon-reload"])?;
    run_systemctl(&["--user", "enable", SERVICE_NAME])?;
    println!("Service enabled. Use `voicerouter service start` to start.");
    Ok(())
}

fn uninstall() -> Result<()> {
    // Stop and remove overlay service first.
    let _ = run_systemctl(&["--user", "stop", OVERLAY_SERVICE_NAME]);
    let _ = run_systemctl(&["--user", "disable", OVERLAY_SERVICE_NAME]);
    let overlay_unit_path = unit_dir()?.join(OVERLAY_SERVICE_UNIT);
    if overlay_unit_path.exists() {
        std::fs::remove_file(&overlay_unit_path).ok();
        println!("Removed: {}", overlay_unit_path.display());
    }

    let _ = run_systemctl(&["--user", "stop", SERVICE_NAME]);
    let _ = run_systemctl(&["--user", "disable", SERVICE_NAME]);

    let unit_path = unit_dir()?.join(SERVICE_UNIT);
    if unit_path.exists() {
        std::fs::remove_file(&unit_path).with_context(|| {
            format!("removing unit file: {}", unit_path.display())
        })?;
        println!("Removed: {}", unit_path.display());
    } else {
        println!("Unit file not found — nothing to remove.");
    }

    let _ = run_systemctl(&["--user", "daemon-reload"]);
    Ok(())
}

fn systemctl(action: &str) -> Result<()> {
    run_systemctl(&["--user", action, SERVICE_NAME])
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let status = std::process::Command::new("systemctl")
        .args(args)
        .status()
        .context("failed to run systemctl")?;

    if !status.success() {
        anyhow::bail!(
            "systemctl {} exited with {}",
            args.join(" "),
            status
        );
    }
    Ok(())
}

fn unit_dir() -> Result<std::path::PathBuf> {
    let config_dir =
        dirs::config_dir().context("cannot determine config directory")?;
    Ok(config_dir.join("systemd").join("user"))
}

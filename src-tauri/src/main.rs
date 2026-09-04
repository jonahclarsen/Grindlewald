use std::process::Command;

fn main() {
    if let Some(exit_code) = grindlewald_lib::privileged::maybe_run_helper() {
        std::process::exit(exit_code);
    }

    if std::env::var_os("GRINDLEWALD_DEV_SUPERVISOR").is_some() {
        let repository = std::env::var("GRINDLEWALD_REPO")
            .expect("GRINDLEWALD_REPO is required in development supervisor mode");
        let pnpm = std::env::var("GRINDLEWALD_PNPM")
            .expect("GRINDLEWALD_PNPM is required in development supervisor mode");
        let runner = format!("{repository}/scripts/dev-app-runner.sh");
        let status = Command::new(pnpm)
            .args(["tauri", "dev", "--runner", &runner])
            .current_dir(repository)
            .env_remove("GRINDLEWALD_DEV_SUPERVISOR")
            .status()
            .expect("failed to start the Grindlewald development watcher");
        std::process::exit(status.code().unwrap_or(1));
    }

    grindlewald_lib::run();
}

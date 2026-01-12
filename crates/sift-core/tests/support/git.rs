use std::process::Command;

const GIT_ENV_OVERRIDES: [&str; 4] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
];

pub fn git_command() -> Command {
    let mut cmd = Command::new("git");
    for key in GIT_ENV_OVERRIDES {
        cmd.env_remove(key);
    }
    cmd
}

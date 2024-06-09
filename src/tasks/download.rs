use std::env::consts::EXE_EXTENSION;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumDiscriminants};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::errors::AppError;
use crate::state::AppStateRef;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};

#[derive(
    Debug, Default, Clone, PartialEq, Eq, Display, EnumDiscriminants, Serialize, Deserialize,
)]
#[strum_discriminants(derive(Display))]
pub enum GalleryDLSource {
    #[default]
    None,
    #[strum_discriminants(strum(to_string = "Twitter Likes"))]
    TwitterLikes { username: String },
    #[strum_discriminants(strum(to_string = "Custom URL"))]
    CustomURL { url: String },
}

#[derive(
    Debug, Default, Clone, PartialEq, Eq, Display, EnumDiscriminants, Serialize, Deserialize,
)]
#[strum_discriminants(derive(Display))]
pub enum GalleryDLLogin {
    #[default]
    None,
    #[strum_discriminants(strum(to_string = "Firefox Cookies"))]
    FirefoxCookies,
    #[strum_discriminants(strum(to_string = "Chrome Cookies"))]
    ChromeCookies,
    #[strum_discriminants(strum(to_string = "Username/Password"))]
    UsernamePassword { username: String, password: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GalleryDLParams {
    pub location: Option<String>,
    pub version: Option<String>,
    pub source: GalleryDLSource,
    pub login: GalleryDLLogin,
    pub cli_arguments: String,
    pub json_config: String,
    pub log_file: Option<String>,
}

impl Default for GalleryDLParams {
    fn default() -> Self {
        Self {
            location: None,
            version: None,
            source: GalleryDLSource::None,
            login: GalleryDLLogin::None,
            cli_arguments: "--write-metadata -o skip=true".to_string(),
            //language=json
            json_config: r#"{
    "extractor": {
        "twitter": {
            "users": "https://twitter.com/{legacy[screen_name]}",
            "text-tweets": true,
            "quoted": true,
            "retweets": true,
            "logout": true,
            "replies":"self",
            "filename": "twitter_{author[name]}_{tweet_id}_{num}.{extension}",
            "parent-directory": true,
            "postprocessors": [
                {
                    "name": "metadata",
                    "event": "post",
                    "filename": "twitter_{author[name]}_{tweet_id}_main.json"
                }
            ]
        }
    }
}"#
            .to_string(),
            log_file: None,
        }
    }
}

impl GalleryDLParams {
    pub fn task_name(&self) -> String {
        match &self.source {
            GalleryDLSource::None => String::new(),
            GalleryDLSource::TwitterLikes { username } => {
                format!("Downloading Twitter likes of @{username} using gallery-dl")
            }
            GalleryDLSource::CustomURL { url } => {
                format!("Downloading from {url} using gallery-dl")
            }
        }
    }

    pub fn source_url(&self) -> String {
        match &self.source {
            GalleryDLSource::None => String::new(),
            GalleryDLSource::TwitterLikes { username } => {
                format!("https://twitter.com/{username}/likes")
            }
            GalleryDLSource::CustomURL { url } => url.to_string(),
        }
    }
}

async fn get_first_line(cmd: &mut Command) -> anyhow::Result<String> {
    let cmd_debug = format!("{cmd:?}");
    let output = cmd.output().await?;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(anyhow!(AppError::CommandError {
            command: cmd_debug,
            error: stderr
        }));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|_| {
        anyhow!(AppError::InvalidUnicode).context(format!("while executing command {cmd_debug}"))
    })?;

    let Some(first_line) = stdout.lines().next() else {
        return Err(anyhow!(AppError::CommandError {
            command: cmd_debug,
            error: stderr
        }));
    };

    Ok(first_line.to_string())
}

pub async fn find_gallery_dl(_state: AppStateRef, _progress: ProgressSenderRef) -> AsyncTaskReturn {
    #[cfg(windows)]
    let path = get_first_line(Command::new("where.exe").arg("gallery-dl")).await?;
    #[cfg(not(windows))]
    let path = get_first_line(Command::new("which").arg("gallery-dl")).await?;

    check_gallery_dl(path).await
}

pub async fn select_gallery_dl(
    _state: AppStateRef,
    _progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new().add_filter("Executable", &[EXE_EXTENSION]);

    let fp = dialog.pick_file().await.ok_or(AppError::UserCancelled)?;

    let path = fp
        .path()
        .to_str()
        .ok_or(AppError::InvalidUnicode)?
        .to_string();

    check_gallery_dl(path).await
}

async fn check_gallery_dl(path: String) -> AsyncTaskReturn {
    if !get_first_line(Command::new(path.as_str()).arg("--help"))
        .await?
        .contains("gallery-dl")
    {
        return Err(anyhow!(AppError::UnexpectedExecutable {
            expected: "gallery-dl".to_string(),
            got: path
        }));
    }

    let version = get_first_line(Command::new(path.as_str()).arg("--version")).await?;

    Ok(AsyncTaskResult::FoundGalleryDl { path, version })
}

async fn produce_lines_as_progress(
    stdout: ChildStdout,
    progress: ProgressSenderRef,
    tee: Arc<Mutex<Vec<String>>>,
) -> std::io::Result<()> {
    let stdout = BufReader::new(stdout);
    let mut lines = stdout.lines();
    while let Some(line) = lines.next_line().await? {
        let mut l = tee.lock().await;
        l.push(line.clone());
        if l.len() % 10 == 0 {
            tracing::info!("# lines: {}", l.len());
        }
        progress.send(ProgressState::DeterminateWithMessage(0.0, line));
    }
    Ok(())
}

async fn async_tee(
    stream: impl AsyncRead + Unpin,
    tee: Arc<Mutex<Vec<String>>>,
) -> std::io::Result<()> {
    let buf = BufReader::new(stream);
    let mut lines = buf.lines();
    while let Some(line) = lines.next_line().await? {
        tracing::error!("{}", line.clone());
        tee.lock().await.push(line);
    }
    Ok(())
}

#[allow(clippy::module_name_repetitions)]
pub async fn perform_gallery_dl_download(
    state: AppStateRef,
    progress: ProgressSenderRef,
    params: GalleryDLParams,
) -> AsyncTaskReturn {
    let dl_progress = progress.sub_task("Download", 0.5);
    dl_progress.send(ProgressState::Determinate(0.0));

    let Some(prog) = &params.location else {
        return Err(anyhow!(AppError::MissingExecutable {
            expected: "gallery-dl".to_string(),
        }));
    };

    let mut cmd = Command::new(prog.as_str());
    cmd.arg(params.source_url());
    cmd.arg("--directory")
        .arg(state.read().await.current_vault()?.root_dir()?);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    match &params.login {
        GalleryDLLogin::None => &mut cmd,
        GalleryDLLogin::FirefoxCookies => cmd.arg("--cookies-from-browser").arg("firefox"),
        GalleryDLLogin::ChromeCookies => cmd.arg("--cookies-from-browser").arg("chrome"),
        GalleryDLLogin::UsernamePassword { username, password } => cmd
            .arg("--username")
            .arg(username)
            .arg("--password")
            .arg(password),
    };

    let mut tmp_config_path = std::env::temp_dir();
    tmp_config_path.push(format!("{}.json", Uuid::new_v4()));
    tokio::fs::write(tmp_config_path.clone(), params.json_config).await?;
    cmd.arg("--config-ignore")
        .arg("--config")
        .arg(tmp_config_path);

    cmd.raw_arg(params.cli_arguments);

    let cmd_debug = format!("{cmd:?}");

    let mut child_process = cmd.spawn()?;
    let mut join_set: JoinSet<std::io::Result<Option<ExitStatus>>> = JoinSet::new();

    let stdout = child_process.stdout.take().unwrap();
    let stderr = child_process.stderr.take().unwrap();
    let stdout_tee: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let stderr_tee: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

    join_set.spawn(async move { Ok(Some(child_process.wait().await?)) });
    join_set.spawn(async move {
        produce_lines_as_progress(stdout, dl_progress, stdout_tee).await?;
        Ok(None)
    });
    join_set.spawn(async move {
        async_tee(stderr, stderr_tee).await?;
        Ok(None)
    });

    let status = loop {
        match join_set.join_next().await.expect("non-empty JoinSet")?? {
            None => {}
            Some(status) => break status,
        }
    };
    join_set.shutdown().await;

    if !status.success() {
        return Err(anyhow!(AppError::CommandError {
            command: cmd_debug,
            error: format!(
                "error code {}",
                status
                    .code()
                    .map_or("unknown".to_string(), |c| c.to_string())
            )
        }));
    }

    Ok(AsyncTaskResult::None)
}

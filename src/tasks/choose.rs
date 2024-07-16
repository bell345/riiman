use crate::errors::AppError;
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn};

pub async fn choose_folder() -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new();
    match dialog.pick_folder().await {
        Some(fp) => Ok(AsyncTaskResult::SelectedDirectory(
            fp.path().to_string_lossy().into(),
        )),
        None => Err(AppError::UserCancelled.into()),
    }
}

pub async fn choose_archive() -> AsyncTaskReturn {
    let dialog = rfd::AsyncFileDialog::new()
        .add_filter("ZIP archive", &["zip"])
        .add_filter("7Z archive", &["7z"])
        .add_filter("tar.gz archive", &["tar.gz"]);

    match dialog.pick_file().await {
        Some(fp) => Ok(AsyncTaskResult::SelectedFile(
            fp.path().to_string_lossy().into(),
        )),
        None => Err(AppError::UserCancelled.into()),
    }
}

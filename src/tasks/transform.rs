use crate::tasks::image::{read_image, wand_to_image};
use crate::tasks::{AsyncTaskResult, AsyncTaskReturn, ProgressSenderRef, ProgressState};
use std::path::Path;
use tokio::task::block_in_place;

pub fn load_transformed_image_preview(
    abs_path: impl AsRef<Path>,
    progress: ProgressSenderRef,
) -> AsyncTaskReturn {
    progress.send(ProgressState::Indeterminate);
    block_in_place(|| {
        let wand = read_image(abs_path)?;
        let image = wand_to_image(&wand)?;
        Ok(AsyncTaskResult::PreviewReady { image })
    })
}

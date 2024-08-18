use eframe::egui::Color32;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImageParams {
    pub scale: ScaleOptions,
    pub infill: InfillOptions,
    pub compression: CompressionOptions,
    pub dry_run: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BulkParams {
    pub source: SourceOptions,
    pub destination: DestinationOptions,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum SourceKind {
    #[default]
    #[display("Currently selected images")]
    Selection,
    #[display("All images which match filters")]
    Filtered,
    #[display("All images in vault")]
    All,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceOptions {
    pub kind: SourceKind,
    pub delete_source: bool,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum DestinationKind {
    #[default]
    #[display("Same vault")]
    SameVault,
    #[display("Different vault")]
    OtherVault,
    Directory,
    Archive,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum DestinationExistingBehaviour {
    #[default]
    #[display("Skip over source item")]
    Skip,
    #[display("Remove source item")]
    Remove,
    #[display("Overwrite destination item")]
    Overwrite,
    #[display("Append discriminator to path")]
    AppendDiscriminator,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DestinationOptions {
    pub kind: DestinationKind,
    pub vault_subdirectory: String,
    pub other_vault_name: String,
    pub directory_path: String,
    pub archive_path: String,
    pub use_subdirectory: bool,
    pub archive_existing_behaviour: DestinationExistingBehaviour,
    pub item_existing_behaviour: DestinationExistingBehaviour,
    pub preserve_directory_structure: bool,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScaleOptions {
    pub enabled: bool,
    pub use_target_width: bool,
    pub target_width: u32,
    pub use_target_height: bool,
    pub target_height: u32,
    pub scale_algorithm: ScaleAlgorithm,
    pub esrgan_model: EsrganModel,
    pub integer_scaling: bool,
    pub scale_down: bool,
    pub use_maximum_scaling: bool,
    pub maximum_scaling: OrderedFloat<f32>,
    pub fit_algorithm: FitAlgorithm,
}

impl Default for ScaleOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            use_target_width: false,
            target_width: 1920,
            use_target_height: false,
            target_height: 1080,
            scale_algorithm: Default::default(),
            esrgan_model: Default::default(),
            integer_scaling: false,
            scale_down: false,
            use_maximum_scaling: false,
            maximum_scaling: Default::default(),
            fit_algorithm: Default::default(),
        }
    }
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum ScaleAlgorithm {
    #[default]
    #[display("Nearest Neighbour")]
    NearestNeighbour,
    Bilinear,
    Bicubic,
    #[display("xBRZ")]
    Xbrz,
    #[display("ESRGAN")]
    Esrgan,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum EsrganModel {
    #[default]
    #[display("RealESRGAN")]
    RealEsrgan,
    #[display("RealESRGAN Anime")]
    RealEsrganAnime,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum FitAlgorithm {
    Fill,
    #[default]
    Fit,
    Stretch,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InfillOptions {
    pub enabled: bool,
    pub target_aspect_ratio: (OrderedFloat<f32>, OrderedFloat<f32>),
    pub technique: InfillTechnique,
    pub use_auto_solid: bool,
    pub manual_solid_colour: Color32,
    pub use_gaussian: bool,
    pub gaussian_radius: u32,
    pub use_brightness: bool,
    pub brightness_change: OrderedFloat<f32>,
    pub use_contrast: bool,
    pub contrast_change: OrderedFloat<f32>,
}

impl Default for InfillOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            target_aspect_ratio: (16.0.into(), 9.0.into()),
            technique: Default::default(),
            use_auto_solid: true,
            manual_solid_colour: Color32::BLACK,
            use_gaussian: true,
            gaussian_radius: 12,
            use_brightness: true,
            brightness_change: (-0.5).into(),
            use_contrast: true,
            contrast_change: (-0.5).into(),
        }
    }
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum InfillTechnique {
    #[default]
    Automatic,
    Blur,
    Solid,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompressionOptions {
    pub enabled: bool,
    pub file_type: CompressionFileType,
    pub quality: u16,
    pub chroma_subsampling: ChromaSubsampling,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            file_type: Default::default(),
            quality: 90,
            chroma_subsampling: Default::default(),
        }
    }
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum CompressionFileType {
    #[default]
    #[display("JPEG")]
    Jpeg,
    #[display("PNG")]
    Png,
    #[display("WEBP")]
    Webp,
    #[display("AVIF")]
    Avif,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, derive_more::Display,
)]
pub enum ChromaSubsampling {
    #[display("4:4:4 (Best quality)")]
    Chroma444,
    #[display("4:4:0")]
    Chroma440,
    #[default]
    #[display("4:2:2")]
    Chroma422,
    #[display("4:2:0 (Best compression)")]
    Chroma420,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathParams {
    pub format: String,
    pub dry_run: bool,
}

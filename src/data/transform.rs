use eframe::egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Params {
    pub source_type: SourceType,
    pub source_options: SourceOptions,
    pub destination_type: DestinationType,
    pub destination_options: DestinationOptions,

    pub scale_options: ScaleOptions,
    pub infill_options: InfillOptions,
    pub compression_options: CompressionOptions,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    #[default]
    Selection,
    Filtered,
    All,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SourceOptions {
    pub delete_source: bool,
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DestinationType {
    #[default]
    SameVault,
    OtherVault,
    Directory,
    Archive,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DestinationOptions {
    pub vault_subdirectory: String,
    pub other_vault_name: String,
    pub directory_path: String,
    pub archive_path: String,
    pub use_subdirectory: bool,
    pub replace_archive_if_existing: bool,
    pub replace_items_if_existing: bool,
    pub preserve_directory_structure: bool,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
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
    pub maximum_scaling: f32,
    pub fit_algorithm: FitAlgorithm,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
pub enum ScaleAlgorithm {
    #[default]
    #[display("Nearest Neighbour")]
    NearestNeighbour,
    Bilinear,
    Bicubic,
    #[display("HQx")]
    Hqx,
    #[display("xBR")]
    Xbr,
    #[display("ESRGAN")]
    Esrgan,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
pub enum EsrganModel {
    #[default]
    #[display("RealESRGAN")]
    RealEsrgan,
    #[display("RealESRGAN Anime")]
    RealEsrganAnime,
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
pub enum FitAlgorithm {
    Fill,
    #[default]
    Fit,
    Stretch,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfillOptions {
    pub enabled: bool,
    pub target_aspect_ratio: (f32, f32),
    pub technique: InfillTechnique,
    pub use_auto_solid: bool,
    pub manual_solid_colour: Color32,
    pub use_gaussian: bool,
    pub gaussian_radius: u32,
    pub use_brightness: bool,
    pub brightness_change: f32,
    pub use_contrast: bool,
    pub contrast_change: f32,
}

impl Default for InfillOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            target_aspect_ratio: (16.0, 9.0),
            technique: Default::default(),
            use_auto_solid: true,
            manual_solid_colour: Color32::BLACK,
            use_gaussian: true,
            gaussian_radius: 12,
            use_brightness: true,
            brightness_change: -0.5,
            use_contrast: true,
            contrast_change: -0.5,
        }
    }
}

#[derive(
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
)]
pub enum InfillTechnique {
    #[default]
    Automatic,
    Blur,
    Solid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
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
    Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::Display,
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

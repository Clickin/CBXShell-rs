pub use cbxshell_core::utils;

pub mod decoder {
    pub use cbxshell_image_codec::{decode_image, decode_rgba_image, DecodedRgbaImage};
}

pub mod hbitmap {
    pub use cbxshell_bitmap_win::*;
}

pub mod resizer {
    pub use cbxshell_image_resize::*;
}

pub mod preview_codec {
    pub use cbxshell_image_codec::preview::*;
}

#[path = "../../../CBXShell/src/image_processor/thumbnail.rs"]
pub mod thumbnail;

pub use resizer::ResizeFilter;
pub use thumbnail::{
    create_thumbnail, create_thumbnail_with_size, render_canonical_proxy, render_thumbnail,
    RenderedThumbnail, ThumbnailConfig, RENDERER_REVISION,
};

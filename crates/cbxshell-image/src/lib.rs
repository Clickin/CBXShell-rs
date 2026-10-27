pub mod magic {
    pub use cbxshell_image_magic::*;
}

pub mod decoder {
    pub use cbxshell_image_codec::*;
}

pub mod hbitmap {
    pub use cbxshell_bitmap_win::*;
}

pub mod resizer {
    pub use cbxshell_image_resize::*;
}

pub mod thumbnail {
    pub use cbxshell_thumbnail_win::thumbnail::*;
}

pub use cbxshell_image_magic::{
    detect_image_format, is_image_file, verify_image_data, verify_image_format, ImageFormat,
};
pub use cbxshell_thumbnail_win::{
    create_thumbnail, create_thumbnail_with_size, ResizeFilter, ThumbnailConfig,
};

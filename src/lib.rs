mod image_tiles;
pub mod input;
pub mod markdown;
pub mod math;
mod math_delimiters;
mod math_refs;
mod math_tree;
pub mod output;
pub mod render;
pub mod world;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error>>;
